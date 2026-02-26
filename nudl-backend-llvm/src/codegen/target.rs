use super::*;

/// Embedded pre-compiled runtime object file (built by build.rs).
pub(super) const RUNTIME_OBJ: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/nudl_rt.o"));

pub(super) fn create_target_machine(
    opt_level: OptimizationLevel,
) -> Result<TargetMachine, BackendError> {
    Target::initialize_all(&InitializationConfig::default());

    let target_triple = TargetMachine::get_default_triple();
    let target =
        Target::from_triple(&target_triple).map_err(|e| BackendError::LlvmError(e.to_string()))?;

    target
        .create_target_machine(
            &target_triple,
            "generic",
            "",
            opt_level,
            RelocMode::PIC,
            CodeModel::Default,
        )
        .ok_or_else(|| BackendError::LlvmError("failed to create target machine".into()))
}

pub(super) fn link(obj_path: &Path, rt_obj_path: &Path, output: &Path) -> Result<(), BackendError> {
    let mut cmd = Command::new("cc");
    cmd.arg("-g")
        .arg("-o")
        .arg(output)
        .arg(obj_path)
        .arg(rt_obj_path);

    if cfg!(target_os = "macos") {
        cmd.arg("-lSystem");
    } else {
        cmd.arg("-lc");
    }

    let status = cmd.status()?;

    if !status.success() {
        return Err(BackendError::LinkError(format!(
            "linker exited with status: {}",
            status
        )));
    }

    Ok(())
}
