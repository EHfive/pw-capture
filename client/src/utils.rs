use std::fs;

fn get_program_name() -> Option<String> {
    let program = fs::read_link("/proc/self/exe").ok()?;
    let basename = program.file_name()?;
    Some(basename.to_string_lossy().to_string())
}

fn get_command_name() -> Option<String> {
    let mut f = fs::read_to_string("/proc/self/comm").ok()?;
    f.pop();
    Some(f)
}

pub fn get_app_name() -> String {
    if let Some(program_name) = get_program_name() {
        match program_name.as_str() {
            "wine-preloader" | "wine64-preloader" => (),
            _ => return program_name,
        }
    }
    get_command_name().unwrap_or_else(|| String::from("unknown"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn program_name() {
        let res = get_program_name();
        assert_ne!(res, None);
    }

    #[test]
    fn command_name() {
        let res = get_command_name();
        assert_ne!(res, None);
    }

    #[test]
    fn app_name() {
        let res = get_app_name();
        assert!(!res.is_empty())
    }
}
