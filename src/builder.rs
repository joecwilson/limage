use crate::config::LimageConfig;
use std::{
    path::Path,
    process::{Command, Stdio},
};
use thiserror::Error;

pub struct Builder {
    config: LimageConfig,
}

impl Builder {
    pub fn new(config: LimageConfig) -> Result<Self, BuildError> {
        Ok(Self { config })
    }

    pub fn build(&self, kernel_path: Option<&Path>) -> Result<(), BuildError> {
        self.execute_prebuilder().unwrap();
        self.prepare_ovmf_files().unwrap();
        self.prepare_limine_files().unwrap();
        self.copy_kernel(kernel_path).unwrap();
        self.create_limine_iso().unwrap();
        Ok(())
    }

    fn execute_prebuilder(&self) -> Result<(), BuildError> {
        if let Some(cmd) = &self.config.build.prebuilder {
            Command::new("sh")
                .arg("-c")
                .arg(cmd)
                .stdout(Stdio::piped())
                .output()
                .map_err(|e| BuildError::PrebuilderFailed { source: e })?;
        }
        Ok(())
    }

    fn prepare_ovmf_files(&self) -> Result<(), BuildError> {
        std::fs::create_dir_all(&self.config.build.ovmf_path)?;

        for arch in &["x86_64"] {
            for kind in &["code", "vars"] {
                let url = format!(
                    "https://github.com/osdev0/edk2-ovmf-nightly/releases/latest/download/ovmf-{}-{}.fd",
                    kind, arch
                );
                let path = self
                    .config
                    .build
                    .ovmf_path
                    .join(format!("ovmf-{}-{}.fd", kind, arch));

                Command::new("curl")
                    .arg("-Lo")
                    .arg(&path)
                    .arg(&url)
                    .stdout(Stdio::piped())
                    .output()
                    .map_err(|e| BuildError::DownloadOvmfFailed { source: e })?;
            }
        }
        Ok(())
    }

    fn prepare_limine_files(&self) -> Result<(), BuildError> {
        self.clone_limine_binary()?;
        self.copy_limine_config()?;
        self.copy_limine_binary()?;
        Ok(())
    }

    fn clone_limine_binary(&self) -> Result<(), BuildError> {
        if !self.config.build.limine_path.exists() {
            std::fs::create_dir_all(&self.config.build.limine_path)?; // Create first
            Command::new("git")
                .args(&[
                    "clone",
                    "https://github.com/limine-bootloader/limine.git",
                    "--branch=v8.x-binary",
                    "--depth=1",
                ])
                .arg(&self.config.build.limine_path)
                .stdout(Stdio::piped())
                .status()
                .map_err(|e| BuildError::CloneLimineFailed { source: e })?;
        }
        let paths = std::fs::read_dir(".").unwrap();
        for file in paths {
            println!("File {:?}", file.unwrap());
        }
        // std::env::set_current_dir("limine").unwrap();
        Command::new("make")
            .arg("-C")
            .arg(&self.config.build.limine_path)
            .status()
            .map_err(|e| BuildError::CloneLimineFailed { source: (e) })?;
        // std::env::set_current_dir("..").unwrap();
        Ok(())
    }

    fn copy_limine_config(&self) -> Result<(), BuildError> {
        let config_dir = self.config.build.iso_root.join("boot").join("limine");
        std::fs::create_dir_all(&config_dir)?;

        std::fs::copy("limine.conf", config_dir.join("limine.conf"))
            .map_err(|e| BuildError::CopyLimineConfig { source: e })?;

        Ok(())
    }

    fn copy_limine_binary(&self) -> Result<(), BuildError> {
        let limine_boot_dir = self.config.build.iso_root.join("boot").join("limine");
        let limine_efi_dir = self.config.build.iso_root.join("EFI").join("BOOT");
        println!("{limine_boot_dir:?} {limine_efi_dir:?}");
        std::fs::create_dir_all(&limine_boot_dir)?;
        std::fs::create_dir_all(&limine_efi_dir)?;

        // Copy BIOS files
        for file in &[
            "limine-bios.sys",
            "limine-bios-cd.bin",
            "limine-uefi-cd.bin",
        ] {
            std::fs::copy(
                self.config.build.limine_path.join(file),
                limine_boot_dir.join(file),
            )
            .map_err(|e| BuildError::CopyLimineBinary {
                file: file.to_string(),
                source: e,
            })?;
        }

        // Copy UEFI files
        for file in &["BOOTX64.EFI", "BOOTIA32.EFI"] {
            std::fs::copy(
                self.config.build.limine_path.join(file),
                limine_efi_dir.join(file),
            )
            .map_err(|e| BuildError::CopyLimineBinary {
                file: file.to_string(),
                source: e,
            })?;
        }

        Ok(())
    }

    fn copy_kernel(&self, kernel_path: Option<&Path>) -> Result<(), BuildError> {
        let kernel_dir = self.config.build.iso_root.join("boot").join("kernel");
        std::fs::create_dir_all(&kernel_dir)?;

        let kernel_binary =
            kernel_path.unwrap_or_else(|| Path::new("target/x86_64-unknown-none/debug/kernel"));

        std::fs::copy(kernel_binary, kernel_dir.join("kernel"))
            .map_err(|e| BuildError::CopyKernel { source: e })?;

        Ok(())
    }

    fn create_limine_iso(&self) -> Result<(), BuildError> {
        // Create parent directory for the ISO if it doesn't exist
        if let Some(parent) = self.config.build.image_path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }

        self.create_raw_iso().unwrap();
        self.install_limine_to_iso().unwrap();
        Ok(())
    }

    fn create_raw_iso(&self) -> Result<(), BuildError> {
        Command::new("xorriso")
            .args(&[
                "-as",
                "mkisofs",
                "-b",
                "boot/limine/limine-bios-cd.bin",
                "-no-emul-boot",
                "-boot-load-size",
                "4",
                "-boot-info-table",
                "--efi-boot",
                "boot/limine/limine-uefi-cd.bin",
                "-efi-boot-part",
                "--efi-boot-image",
                "--protective-msdos-label",
            ])
            .arg(&self.config.build.iso_root)
            .arg("-o")
            .arg(&self.config.build.image_path)
            .stdout(Stdio::piped())
            .output()
            .map_err(|e| BuildError::CreateIso { source: e })?;
        Ok(())
    }

    fn install_limine_to_iso(&self) -> Result<(), BuildError> {
        println!("{:?}", self.config.build.limine_path);
        println!("Image path {:?}", self.config.build.image_path);
        let limine_binary = self.config.build.limine_path.join("limine");
        Command::new(limine_binary)
            .args(&[
                "bios-install",
                &self.config.build.image_path.display().to_string(),
            ])
            .stdout(Stdio::piped())
            .output()
            .map_err(|e| BuildError::InstallLimine { source: e })?;
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum BuildError {
    #[error("Failed to locate Cargo.toml")]
    LocateManifest(#[from] locate_cargo_manifest::LocateManifestError),

    #[error("Failed to execute prebuilder command: {source}")]
    PrebuilderFailed { source: std::io::Error },

    #[error("Failed to download OVMF firmware: {source}")]
    DownloadOvmfFailed { source: std::io::Error },

    #[error("Failed to clone Limine repository: {source}")]
    CloneLimineFailed { source: std::io::Error },

    #[error("Failed to copy Limine config: {source}")]
    CopyLimineConfig { source: std::io::Error },

    #[error("Failed to copy Limine binary {file}: {source}")]
    CopyLimineBinary {
        file: String,
        source: std::io::Error,
    },

    #[error("Failed to copy kernel binary: {source}")]
    CopyKernel { source: std::io::Error },

    #[error("Failed to create ISO: {source}")]
    CreateIso { source: std::io::Error },

    #[error("Failed to install Limine to ISO: {source}")]
    InstallLimine { source: std::io::Error },

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
