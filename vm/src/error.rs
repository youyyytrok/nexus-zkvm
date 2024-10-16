pub use nexus_common::error::*;

use nexus_common::riscv::Opcode;
use thiserror::Error;

/// Errors related to VM operations.
#[derive(Debug, Error, PartialEq)]
pub enum VMError {
    // Unimplemented syscall.
    #[error("Unimplemented syscall: opcode=0x{0:08X}, pc=0x{1:08X}")]
    UnimplementedSyscall(u32, u32),

    // Invalid memory layout.
    #[error("Invalid memory layout")]
    InvalidMemoryLayout,

    // VM has run out of instructions to execute.
    #[error("VM has run out of instructions to execute")]
    VMOutOfInstructions,

    // VM has exited with status code.
    #[error("VM has exited with status code {0}")]
    VMExited(u32),

    // Invalid Profile Label.
    #[error("Invalid profile label for cycle counter: \"{0}\"")]
    InvalidProfileLabel(String),

    #[error("Wrapped MemoryError: {0}")]
    MemoryError(#[from] nexus_common::error::MemoryError),

    #[error("Wrapped OpcodeError: {0}")]
    OpcodeError(#[from] nexus_common::error::OpcodeError),

    #[error("Instruction not found in registry")]
    InstructionNotFound,

    // Duplicate Opcode and Instruction.
    #[error("Duplicate Opcode/Instruction in registry")]
    DuplicateInstruction(Opcode),

    // Unimplemented instruction (with a valid opcode).
    #[error("Unimplemented instruction \"{0:08X}\"")]
    UnimplementedInstruction(u32),

    // Unimplemented instruction (with a valid opcode) found at a specific PC.
    #[error("Unimplemented instruction \"{1:08X}\" at pc=0x{1:08X}")]
    UnimplementedInstructionAt(u32, u32),

    // Unsupported instruction (i.e., one with an invalid opcode).
    #[error("Unsupported instruction \"{0:08X}\"")]
    UnsupportedInstruction(u32),
}

/// Result type for VM functions that can produce errors.
pub type Result<T, E = VMError> = std::result::Result<T, E>;
