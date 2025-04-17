use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::env;
use std::fs;
use std::ffi::CString;
use std::fs::OpenOptions;
use serde::{Serialize, Deserialize};

use std::io::Write;
use log::{LevelFilter, info, debug};

use anyhow::{bail, Context, Result};
use std::process::Command;

mod triple;
use triple::Triple;

#[derive(Debug, Parser)]
struct Args {
    #[arg(short, long, default_value = "info")]
    loglevel: LevelFilter,
    #[command(subcommand)]
    cmd: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Setup directory structure
    Setup,
    /// Operations on a toolchain
    Toolchain {
        /// Target triple
        target: Triple,
        #[command(subcommand)]
        cmd: TargetCmd,
    },
    /// Show current config
    Show,
    /// Remove everything that chained has installed
    Remove,
}

#[derive(Debug, Subcommand)]
enum TargetCmd {
    /// Configure, download and build a target toolchain
    Add {
        /// Git source URL for GCC
        #[arg(short, long, default_value = "https://github.com/rust-lang/gcc.git")]
        gcc_src: String,
        /// Inspect config with `nconfig`
        #[arg(short, long)]
        inspect: bool,
    },
    /// Show information about the toolchain
    Show,
    /// Download everything required to compile
    Download,
    /// Compile the toolchain
    Compile,
    /// Reconfigure the toolchain with nconfig
    Reconfigure,
    /// Start a shell with environment set up for cross compilation
    Shell,
}

#[derive(Debug, Serialize, Deserialize)]
struct Toolchain {
    triple: Triple,
    gcc_src: String,
    basedir: PathBuf,
    json_spec: PathBuf,
    prefix: PathBuf,
}

impl Toolchain {
    pub fn crosstool_config(&self, cfg: &Config) -> String {
        let mut opts = Vec::new();
        self.triple.emit_crosstool_config(&mut opts);

        opts.push(format!("CT_LOCAL_TARBALLS_DIR=\"{}\"", cfg.cache_dir.display()));
        opts.push(format!("CT_PREFIX_DIR=\"{}\"", self.prefix.display()));

        opts.push(String::from("CT_GCC_SRC_DEVEL=y"));
        opts.push(format!("CT_GCC_DEVEL_URL=\"{}\"", self.gcc_src));

        opts.push(String::from("CT_CC_LANG_JIT=y"));
        opts.push(String::from("CT_EXPERIMENTAL=y"));
        opts.push(String::from("CT_CC_GCC_EXTRA_CONFIG_ARRAY=\"--enable-host-shared --disable-bootstrap\""));

        opts.into_iter().map(|v| v + "\n").collect()
    }
    fn env_vars(&self) -> Result<Vec<CString>> {
        use std::ffi::CString;

        let bin_dir = self.prefix.join("bin");
        let path = env::var("PATH")
            .unwrap_or_default();
        let path = format!("PATH={}:{}", bin_dir.display(), path);

        let lib_dir = self.prefix.join("lib");
        let ld_path = if let Some(ld) = env::var("LD_LIBRARY_PATH").ok() {
            format!("LD_LIBRARY_PATH={}:{}", lib_dir.display(), ld)
        } else {
            format!("LD_LIBRARY_PATH={}", lib_dir.display())
        };

        let qemu_ld_prefix = self.prefix
            .join(self.triple.to_string())
            .join("sysroot");
        let qemu_ld_prefix = format!("QEMU_LD_PREFIX={}", qemu_ld_prefix.display());

        let triple_for_env = self.triple.to_string().replace('-', "_").to_uppercase();
        let set_linker = format!("CARGO_TARGET_{}_LINKER={}-gcc", triple_for_env, self.triple.to_string());

        Ok(vec![
            CString::new(path)?,
            CString::new(ld_path)?,
            CString::new(qemu_ld_prefix)?,
            CString::new(set_linker)?,
        ])
    }
    fn shell(&self) -> Result<()> {
        use std::ffi::CString;
        let mut env = self.env_vars()?;

        let shell: CString = CString::new(env::var("SHELL")
            .unwrap())?;

        let prompt = CString::new(
            format!(r#"PROMPT_COMMAND=if [ "$SET_PS1" != "true" ]; then SET_PS1=true; PS1="[{}] $PS1"; fi "#, self.triple),
        )?;

        env.push(prompt);

        nix::unistd::execvpe(&shell, &[&shell], &env)
            .context("Failed to exec into shell")?;

        Ok(())

    }
    fn nconfig(&self) -> Result<()> {
        let status = Command::new("ct-ng")
            .arg("nconfig")
            .current_dir(&self.basedir)
            .status()
            .context("Failed to set crosstool config")?;
        if !status.success() {
            if let Some(c) = status.code() {
                bail!("ct-ng nconfig exited with a non-zero status code {c}")
            } else {
                bail!("ct-ng nconfig died")
            }
        }

        Ok(())
    }
    fn defconfig(&self, cfg: &Config) -> Result<()> {
        let ct_cfg = self.crosstool_config(&cfg);

        if !self.basedir.exists() {
            fs::create_dir(&self.basedir)
                .context("Failed to create new target's base directory")?;
        }

        let defconfig_path = self.basedir.join("defconfig");
        log::debug!("Defconfig is at {}", defconfig_path.display());

        fs::write(&defconfig_path, &ct_cfg)
            .context("Failed to write defconfig file")?;

        log::debug!("Running ct-ng defconfig");
        let status = Command::new("ct-ng")
            .arg("defconfig")
            .current_dir(&self.basedir)
            .status()
            .context("Failed to set crosstool config")?;
        if !status.success() {
            if let Some(c) = status.code() {
                bail!("ct-ng defconfig exited with a non-zero status code {c}")
            } else {
                bail!("ct-ng defconfig died")
            }
        }

        Ok(())
    }
    fn compile(&self) -> Result<()> {
        log::info!("Compiling...");
        let status = Command::new("ct-ng")
            .arg("build")
            .current_dir(&self.basedir)
            .status()
            .context("Failed to build toolchain")?;
        if !status.success() {
            if let Some(c) = status.code() {
                bail!("ct-ng build exited with a non-zero status code {c}")
            } else {
                bail!("ct-ng build died")
            }
        }
        Ok(())
    }
}


#[derive(Debug, Serialize, Deserialize)]
struct Config {
    cache_dir: PathBuf,
    data_dir: PathBuf,
    toolchain: Vec<Toolchain>,
}

impl Config {
    fn save(&self) -> Result<()> {
        let to_save = toml::to_string(&self)
            .context("Failed to serialize config")?;
        fs::write(Self::path(), &to_save)
            .context("Failed to save the config")?;
        Ok(())
    }
    fn path() -> PathBuf {
        let dirs = directories::ProjectDirs::from("", "", "chained").unwrap();
        dirs.config_local_dir()
            .join("chained.toml")
    }
    fn load() -> Result<(Config, PathBuf)> {
        let path = Self::path();
        let cfg_string = fs::read_to_string(&path)
            .with_context(|| format!("Failed to open and read config file from {}", path.display()))?;

        let me = toml::from_str(&cfg_string)
            .context("Failed to deserialize config file")?;

        Ok((me, path))
    }
    fn find_toolchain(&self, name: &Triple) -> Option<&Toolchain> {
        self.toolchain.iter()
            .find(|toolchain| toolchain.triple == *name)
    }
}

fn main() -> Result<()> {
    let args = Args::parse();

    env_logger::builder()
        .filter_level(args.loglevel)
        .format(|buf, record| {
            writeln!(buf, "{}: {}", record.level(), record.args())
        })
    .init();

    match args.cmd {
        Commands::Setup => {
            let dirs = directories::ProjectDirs::from("", "", "chained")
                .unwrap();

            let create_dirs = [
                dirs.cache_dir(),
                dirs.data_local_dir(),
                dirs.config_local_dir(),
            ];
            for d in create_dirs.iter() {
                log::debug!("Trying to create {}", d.display());
                if d.exists() {
                    let is_file = if d.is_file() { " and is a file" } else {""};
                    log::warn!("{} already exists{is_file}", d.display());
                    continue;
                }
                fs::create_dir(&d)
                    .with_context(|| format!("Failed to create {} dir", d.display()))?;
            }

            let path = Config::path();
            let config = Config {
                cache_dir: dirs.cache_dir().into(),
                data_dir: dirs.data_local_dir().into(),
                toolchain: Vec::new(),
            };
            let string = toml::to_string(&config)
                .unwrap();
            debug!("Writing config to: {}", path.display());
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path)
                .context("Failed to open file for reading")?;

            file.write_all(string.as_bytes())
                .with_context(|| format!("Failed to write config to {}", path.display()))?;

            Ok(())
        },
        Commands::Remove => {
            let dirs = directories::ProjectDirs::from("", "", "chained")
                .unwrap();

            let remove_dirs = [
                dirs.cache_dir(),
                dirs.data_local_dir(),
                dirs.config_local_dir(),
            ];
            for d in remove_dirs.iter() {
                if d.exists() {
                    if d.is_dir() {
                        log::debug!("Removing {}", d.display());
                        fs::remove_dir_all(&d)
                            .with_context(|| format!("Failed to remove {}", d.display()))?;
                    } else {
                        log::warn!("Not removing {}, not a directory?", d.display());
                    }
                }
            }

            Ok(())
        },
        Commands::Toolchain { target, cmd } => {
            let (cfg, _) = Config::load()
                .context("Failed to load config file, have you tried running setup?")?;
            match cmd {
                TargetCmd::Add { gcc_src, inspect } => {
                    let tgt_dir: PathBuf = target.to_string().into();
                    let basedir: PathBuf = cfg.data_dir.join(tgt_dir);
                    let new = Toolchain {
                        triple: target.clone(),
                        basedir: basedir.clone(),
                        gcc_src,
                        json_spec: basedir.join("target.json"),
                        prefix: basedir.join("prefix"),
                    };

                    let mut cfg = cfg;
                    cfg.toolchain.push(new);
                    cfg.save()
                        .context("Failed to save the new config")?;
                    let new = cfg.find_toolchain(&target).unwrap();

                    log::debug!("Adding {:#?}", new);

                    new.defconfig(&cfg)
                        .context("Failed to configure new toolchain")?;
                    if inspect {
                        new.nconfig()
                            .context("Failed to nconfig new toolchain")?;
                    }
                    new.compile()
                        .context("Failed to compile new toolchain")?;

                    println!("Toolchain {} installed correctly", target);

                    Ok(())
                },
                TargetCmd::Reconfigure => {
                    if let Some(t) = cfg.find_toolchain(&target) {
                        t.nconfig()
                            .context("Failed to nconfig toolchain")?;
                    } else {
                        bail!("Toolchain {} not found", target);
                    }

                    Ok(())
                },
                TargetCmd::Show => {
                    if let Some(t) = cfg.find_toolchain(&target) {
                        println!("Toolchain triple {}:", t.triple);
                        println!("\tJSON target specification path: {}", t.json_spec.display());
                        println!("\tbase directory path: {}", t.basedir.display());
                        println!("\tprefix path: {}", t.prefix.display());
                    } else {
                        bail!("Toolchain {} not found", target);
                    }
                    Ok(())
                },
                TargetCmd::Shell => {
                    if let Some(t) = cfg.find_toolchain(&target) {
                        t.shell()?
                    } else {
                        bail!("Toolchain {} not found", target);
                    }
                    Ok(())
                },
                _ => todo!(),
            }
        },
        Commands::Show => {
            let (cfg, path) = Config::load()
                .context("Failed to load config file, have you tried running setup?")?;

            println!("Read config from {}", path.display());
            println!("Cache directory: {}", cfg.cache_dir.display());
            println!("Data directory: {}", cfg.data_dir.display());
            for tgt in cfg.toolchain.iter() {
                println!();
                println!("Toolchain triple {}:", tgt.triple);
                println!("\tJSON target specification path: {}", tgt.json_spec.display());
                println!("\tbase directory path: {}", tgt.basedir.display());
                println!("\tprefix path: {}", tgt.prefix.display());
            }

            Ok(())
        },
    }
}
