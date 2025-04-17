use bitflags::bitflags;
use strum::EnumString;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub enum Endian {
    Little,
    Big,
}

bitflags! {
    pub struct Bits: u8 {
        const B_8   = 0b0000_0001;
        const B_16  = 0b0000_0010;
        const B_32  = 0b0000_0100;
        const B_64  = 0b0000_1000;
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub enum X86Variant {
    I386,
    I586,
    I686,
    X86_64,
    X86_64h,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub enum Arch {
    // I am not going to parse the clusterfuck of arm32 triples
    Arm64(Endian),
    M68k,
    Mips32(Endian),
    Mips64(Endian),
    //PowerPc32(Endian),
    Sh3(Endian),
    X86(X86Variant),
}

impl Arch {
    fn endian_cfg(&self) -> &'static str {
        match self {
            Self::Arm64(e) | Self::Mips32(e) | Self::Mips64(e) | Self::Sh3(e) => {
                match e {
                    Endian::Little => "CT_ARCH_LE=y",
                    Endian::Big => "CT_ARCH_BE=y",
                }
            },
            Self::M68k | Self::X86(_) => "CT_ARCH_LE=y",
        }
    }
    fn bitness_cfg(&self) -> &'static str {
        match self {
            Self::Arm64(_) | Self::Mips64(_) | Self::X86(X86Variant::X86_64) | Self::X86(X86Variant::X86_64h) => "CT_ARCH_64=y",
            Self::Mips32(_) | Self::Sh3(_) | Self::M68k | Self::X86(_) => "CT_ARCH_32=y"
        }
    }
    fn parse1(s: &mut &str) -> winnow::Result<Self> {
        dispatch! {ident;
            "m68k" => empty.value(Self::M68k),
            "aarch64" => empty.value(Self::Arm64(Endian::Little)),
            "arm64" => empty.value(Self::Arm64(Endian::Little)),
            "aarch64_be" => empty.value(Self::Arm64(Endian::Big)),
            "mipsel" => empty.value(Self::Mips32(Endian::Little)),
            "mips" => empty.value(Self::Mips32(Endian::Big)),
            "mips64" => empty.value(Self::Mips64(Endian::Big)),
            "mips64el" => empty.value(Self::Mips64(Endian::Little)),
            "i386" => empty.value(Self::X86(X86Variant::I386)),
            "i586" => empty.value(Self::X86(X86Variant::I586)),
            "i686" => empty.value(Self::X86(X86Variant::I686)),
            "x86_64" => empty.value(Self::X86(X86Variant::X86_64)),
            "x86_64h" => empty.value(Self::X86(X86Variant::X86_64h)),
            "sh3" => empty.value(Self::Sh3(Endian::Little)),
            _ => fail,
        }.parse_next(s)
    }
    fn emit_crosstool_config(&self, opts: &mut Vec<String>) {
        let arch_cfg = match self {
            Self::Arm64(_) => "CT_ARCH_ARM=y",
            Self::Mips32(_) | Self::Mips64(_) => "CT_ARCH_MIPS=y",
            Self::Sh3(_) => "CT_ARCH_SH=y",
            Self::M68k => "CT_ARCH_M68K=y",
            Self::X86(_) => "CT_ARCH_X86=y",
        };
        opts.push(arch_cfg.into());
        opts.push(self.endian_cfg().into());
        opts.push(self.bitness_cfg().into());
    }
}

impl fmt::Display for Arch {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let s = match self {
            Arch::Arm64(Endian::Little) => "aarch64",
            Arch::Arm64(Endian::Big) => "aarch64_be",
            Arch::M68k => "m68k",
            Arch::Mips32(Endian::Little) => "mipsel",
            Arch::Mips32(Endian::Big) => "mips",
            Arch::Mips64(Endian::Little) => "mips64el",
            Arch::Mips64(Endian::Big) => "mips64",
            Arch::Sh3(Endian::Little) => "sh3",
            Arch::Sh3(Endian::Big) => todo!("sh3 big endian"),
            Arch::X86(v) => match v {
                X86Variant::I386 => "i386",
                X86Variant::I586 => "i586",
                X86Variant::I686 => "i686",
                X86Variant::X86_64 => "x86_64",
                X86Variant::X86_64h => "x86_64h",
            },
        };
        f.write_str(s)
    }
}

use winnow::combinator::{empty, dispatch, fail};

#[derive(Debug, Clone, Eq, PartialEq, EnumString, Serialize, Deserialize, strum::Display)]
#[strum(serialize_all = "lowercase")]
pub enum LinuxLibc {
    Gnu,
    Musl,
    Uclibc,
}

#[derive(Debug, Clone, Eq, PartialEq, EnumString, Serialize, Deserialize, strum::Display)]
#[strum(serialize_all = "lowercase")]
pub enum NoneAbi {
    Elf,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, strum::Display)]
pub enum Os {
    #[strum(to_string = "linux-{0}")]
    Linux(LinuxLibc),
    #[strum(to_string = "none-{0}")]
    None(NoneAbi),
}

impl Os {
    fn emit_crosstool_config(&self, opts: &mut Vec<String>) {
        match self {
            Self::Linux(libc) => {
                opts.push("CT_KERNEL_LINUX=y".into());
                match libc {
                    LinuxLibc::Gnu => opts.push("CT_LIBC_GLIBC=y".into()),
                    LinuxLibc::Musl => opts.push("CT_LIBC_MUSL=y".into()),
                    LinuxLibc::Uclibc => opts.push("CT_LIBC_UCLIBC_NG".into()),
                }
            },
            Self::None(abi) => {
                opts.push("CT_KERNEL_BARE_METAL=y".into());
                match abi {
                    NoneAbi::Elf => (),
                }
            },
        }
    }
    fn parse_osabi(os: &str, abiname: &str) -> winnow::Result<Self> {
        match os {
            "linux" => {
                let libc = abiname.parse().unwrap();
                Ok(Self::Linux(libc))
            },
            "none" | "unknown" => {
                let abi = abiname.parse().unwrap();
                Ok(Self::None(abi))
            },
            _ => panic!(),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct Triple {
    arch: Arch,
    vendor: String,
    os: Os,
}


use winnow::token::{one_of, take_while};
use winnow::Parser;
use winnow::stream::AsChar;
pub fn ident<'a>(input: &mut &'a str) -> winnow::Result<&'a str> {
    (
        one_of(|c: char| c.is_alpha() || c == '_'),
        take_while(0.., |c: char| c.is_alphanum() || c == '_')
    )
        .take()
        .parse_next(input)
}


impl Triple {
    fn parse(s: &mut &str) -> winnow::Result<Triple> {
        use winnow::combinator::separated;
        let v: Vec<&str> = separated(1.., ident, '-')
            .parse_next(s)?;

        let v = match v.as_slice() {
            &[mut arch, os, abi] => Triple {
                arch: Arch::parse1(&mut arch)?,
                vendor: "unknown".into(),
                os: Os::parse_osabi(os, abi)?,
            },
            &[mut arch, vendor, os, abi] => Triple {
                arch: Arch::parse1(&mut arch)?,
                vendor: vendor.to_string(),
                os: Os::parse_osabi(os, abi)?,
            },
            _ => todo!("proper errors, invalid triple length or something"),
        };
        Ok(v)
    }
    pub fn emit_crosstool_config(&self, opts: &mut Vec<String>) {
        self.arch.emit_crosstool_config(opts);
        opts.push(format!("CT_TARGET_VENDOR={}", self.vendor));
        self.os.emit_crosstool_config(opts);
    }
    #[cfg(test)]
    fn new4(arch: Arch, vendor: impl Into<String>, os: Os) -> Self {
        Self {
            arch,
            vendor: vendor.into(),
            os,
        }
    }
    #[cfg(test)]
    fn new3(arch: Arch, os: Os) -> Self {
        Self::new4(arch, "unknown", os)
    }
}

use std::fmt;
impl fmt::Display for Triple {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}-{}-{}", self.arch, self.vendor, self.os)
    }
}

use std::str::FromStr;
impl FromStr for Triple {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Triple::parse.parse(s)
            .map_err(|e| e.to_string())
    }
}


#[cfg(test)]
mod tests {
    use super::{Arch, Os, LinuxLibc, Triple, NoneAbi, Endian};
    use std::str::FromStr;

    #[test]
    fn _if_no_vendor_then_unknown() {
        assert_eq!(
            Triple::from_str("m68k-linux-musl").unwrap(),
            Triple::from_str("m68k-unknown-linux-musl").unwrap()
        );
        assert_eq!(
            Triple::from_str("m68k-linux-gnu").unwrap(),
            Triple::from_str("m68k-unknown-linux-gnu").unwrap()
        );
    }

    #[test]
    fn parse_arm64() {
        let aarch64_linux_gnu = Triple::new3(Arch::Arm64(Endian::Little), Os::Linux(LinuxLibc::Gnu));
        assert_eq!(aarch64_linux_gnu, Triple::from_str("aarch64-linux-gnu").unwrap());
        assert_eq!(aarch64_linux_gnu, Triple::from_str("arm64-linux-gnu").unwrap());
    }

    #[test]
    fn parse_m68k() {
        let m68k_unknown_linux_gnu = Triple::new3(Arch::M68k, Os::Linux(LinuxLibc::Gnu));
        assert_eq!(m68k_unknown_linux_gnu, Triple::from_str("m68k-unknown-linux-gnu").unwrap());

        let m68k_unknown_linux_musl = Triple::new3(Arch::M68k, Os::Linux(LinuxLibc::Musl));
        assert_eq!(m68k_unknown_linux_musl, Triple::from_str("m68k-unknown-linux-musl").unwrap());

        let m68k_unknown_elf = Triple::new3(Arch::M68k, Os::None(NoneAbi::Elf));
        assert_eq!(m68k_unknown_elf, Triple::from_str("m68k-unknown-elf").unwrap());
    }

    #[test]
    fn parse_mips() {
        let mips_linux_gnu = Triple::new3(Arch::Mips32(Endian::Big), Os::Linux(LinuxLibc::Gnu));
        assert_eq!(mips_linux_gnu, Triple::from_str("mips-linux-gnu").unwrap());

        let mipsel_linux_gnu = Triple::new3(Arch::Mips32(Endian::Little), Os::Linux(LinuxLibc::Gnu));
        assert_eq!(mipsel_linux_gnu, Triple::from_str("mipsel-linux-gnu").unwrap());

        let mips64_linux_gnu = Triple::new3(Arch::Mips64(Endian::Big), Os::Linux(LinuxLibc::Gnu));
        assert_eq!(mips64_linux_gnu, Triple::from_str("mips64-linux-gnu").unwrap());

        let mips64el_linux_gnu = Triple::new3(Arch::Mips64(Endian::Little), Os::Linux(LinuxLibc::Gnu));
        assert_eq!(mips64el_linux_gnu, Triple::from_str("mips64el-linux-gnu").unwrap());
    }

    #[test]
    fn parse_superh() {
        let sh3_unknown_elf = Triple::new3(Arch::Sh3(Endian::Little), Os::None(NoneAbi::Elf));
        assert_eq!(sh3_unknown_elf, Triple::from_str("sh3-unknown-elf").unwrap());
    }
}
