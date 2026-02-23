use std::fs;
use std::path::PathBuf;
use std::process::Command;

const WORKSPACE_DIR: &str = "./workspace";

fn run_python_code(code: &str) -> String {
    let clean_code = code
        .trim()
        .trim_start_matches("```python")
        .trim_start_matches("```")
        .trim_end_matches("```");

    println!("OS: Executing Python Code:\n---\n{}\n---", clean_code);
    let temp_filename = "agent_script_temp.py";
    if let Err(e) = std::fs::write(temp_filename, clean_code) {
        return format!("SysCall Error: Failed to write temp file: {}", e);
    }

    let output = Command::new("python3").arg(temp_filename).output();
    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            let result = if !stderr.is_empty() {
                format!("Output:\n{}\nErrors:\n{}", stdout, stderr)
            } else {
                format!("{}", stdout)
            };
            let _ = std::fs::remove_file(temp_filename);

            let max_len = 2000;
            if result.len() > max_len {
                format!("{}... (Output Truncated)", &result[..max_len])
            } else if result.trim().is_empty() {
                "Done (No Output)".to_string()
            } else {
                result.to_string()
            }
        }
        Err(e) => format!("SysCall Error: Failed to execute python3: {}", e),
    }
}

fn resolve_safe_path(filename: &str) -> Option<PathBuf> {
    let clean_name = filename.trim();
    if clean_name.contains("..") || clean_name.starts_with('/') || clean_name.starts_with('\\') {
        return None;
    }
    let mut path = PathBuf::from(WORKSPACE_DIR);
    path.push(clean_name);
    Some(path)
}

fn handle_write_file(args: &str) -> String {
    let parts: Vec<&str> = args.splitn(2, '|').collect();
    if parts.len() < 2 {
        return "SysCall Error: Usage [[WRITE_FILE: filename | content]]".to_string();
    }

    let filename = parts[0].trim();
    let content = parts[1].trim_start();
    if let Err(e) = fs::create_dir_all(WORKSPACE_DIR) {
        return format!("SysCall Error: Failed to create workspace: {}", e);
    }

    if let Some(path) = resolve_safe_path(filename) {
        println!("OS: Writing file {:?}", path);
        match fs::write(&path, content) {
            Ok(_) => format!(
                "Success: File '{}' written ({} bytes).",
                filename,
                content.len()
            ),
            Err(e) => format!("SysCall Error: Write failed: {}", e),
        }
    } else {
        "SysCall Error: Invalid filename or security violation.".to_string()
    }
}

fn handle_read_file(filename: &str) -> String {
    if let Some(path) = resolve_safe_path(filename) {
        println!("OS: Reading file {:?}", path);
        match fs::read_to_string(&path) {
            Ok(content) => content,
            Err(e) => format!("SysCall Error: Read failed: {}", e),
        }
    } else {
        "SysCall Error: Invalid filename or security violation.".to_string()
    }
}

fn handle_list_files() -> String {
    let _ = fs::create_dir_all(WORKSPACE_DIR);
    match fs::read_dir(WORKSPACE_DIR) {
        Ok(entries) => {
            let files: Vec<String> = entries
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect();
            if files.is_empty() {
                "Workspace is empty.".to_string()
            } else {
                format!("Files:\n- {}", files.join("\n- "))
            }
        }
        Err(e) => format!("SysCall Error: LS failed: {}", e),
    }
}

pub fn handle_syscall(command_block: &str) -> String {
    let clean_cmd = command_block.trim();

    if clean_cmd.starts_with("PYTHON:") {
        return run_python_code(clean_cmd.trim_start_matches("PYTHON:"));
    }
    if clean_cmd.starts_with("WRITE_FILE:") {
        return handle_write_file(clean_cmd.trim_start_matches("WRITE_FILE:"));
    }
    if clean_cmd.starts_with("READ_FILE:") {
        return handle_read_file(clean_cmd.trim_start_matches("READ_FILE:").trim());
    }
    if clean_cmd.starts_with("LS") {
        return handle_list_files();
    }
    if clean_cmd.starts_with("CALC:") {
        let expr = clean_cmd.trim_start_matches("CALC:").trim();
        return run_python_code(&format!("print({})", expr));
    }

    "SysCall Error: Unknown Tool.".to_string()
}
