use crate::{
    bytecode::{
        Argument,
        OpCode,
        Function,
        ByteCode,
        CodeGenerator
    },
    types::DataType
};
use std::fmt::{self, Write};

pub fn data_type_generate(data_type: &DataType) -> &str {
    match data_type.size() {
        1 => "byte",
        2 => "word",
        4 => "dword",
        8 => "qword",
        _ => unreachable!(),
    }
}

pub fn is_argument_comparable(argument: &Argument) -> bool {
    !matches!(argument, Argument::Register(_) | Argument::Argument(_))
}

pub enum NasmRegister {
    Rax,
    Rbx,
    Rcx,
    Rdx,
    Rdi,
    Rsi,
    Rsp,
    Rbp,
    R8,
    R9,
    R10,
    R11,
}

impl NasmRegister {
    fn generate(&self, data_type: &DataType) -> &str {
        let text_options = match self {
            Self::Rax => &["al", "ax", "eax", "rax"],
            Self::Rbx => &["bl", "bx", "ebx", "rbx"],
            Self::Rcx => &["cl", "cx", "ecx", "rcx"],
            Self::Rdx => &["dl", "dx", "edx", "rdx"],
            Self::Rsi => &["sil", "si", "esi", "rsi"],
            Self::Rdi => &["dil", "di", "edi", "rdi"],
            Self::Rsp => &["spl", "sp", "esp", "rsp"],
            Self::Rbp => &["bpl", "bp", "ebp", "rbp"],
            Self::R8 => &["r8b", "r8d", "r8w", "r8"],
            Self::R9 => &["r9b", "r9d", "r9w", "r9"],
            Self::R10 => &["r10b", "r10d", "r10w", "r10"],
            Self::R11 => &["r11b", "r11d", "r11w", "r11"],
        };

        text_options[match data_type.size() {
            1 => 0,
            2 => 1,
            4 => 2,
            8 => 3,
            _ => unreachable!(),
        }]
    }
}

const PRINT_CODE: &str = "\
print:
    enter 0, 0
    mov rax, 1
    mov rdi, 1
    mov rsi, [rbp + 24]
    mov rdx, [rbp + 16]
    syscall
    leave
    ret
";

const ENTRY_CODE: &str = "\
_start:
    sub rsp, 8
    call @main
    mov rax, 60
    pop rdi
    syscall
";

pub struct Nasm {
    text: String,
}

impl Nasm {
    fn generate_argument<'src, 'a>(&mut self, function: &'a Function<'src>, argument: &Argument) -> Result<String, fmt::Error> {
        let text = match argument {
            Argument::ReturnValue => {
                format!(
                    "{} [rbp + {}]",
                    data_type_generate(&function.return_type),
                    8 + function.arguments_size + function.return_type.size_aligned()
                )
            }
            Argument::Register(register_id) => {
                format!(
                    "{} [rbp - {}]",
                    data_type_generate(&function.register_types[*register_id]),
                    8 + function.register_position(*register_id)
                )
            }
            Argument::Argument(argument_id) => {
                format!(
                    "{} [rbp + {}]",
                    data_type_generate(&function.argument_types[*argument_id]),
                    8 + function.arguments_size - function.argument_position(*argument_id)
                )
            }
            Argument::Constant { value, .. } => value.to_string(),
            Argument::Symbol { name, .. } => name.clone(),
            Argument::VoidRegister => unreachable!(),
        };

        Ok(text)
    }

    fn generate_infix<'src, 'a>(
        &mut self,
        function: &'a Function<'src>,
        dst: &Argument,
        src: &Argument,
        operation: &'a str
    ) -> fmt::Result {
        let rax = NasmRegister::Rax.generate(function.argument_data_type(dst));

        let src_compiled = self.generate_argument(function, src)?;
        let dst_compiled = self.generate_argument(function, dst)?;

        if is_argument_comparable(src) {
            writeln!(self.text, "    {operation} {dst_compiled}, {src_compiled}")
        } else {
            writeln!(self.text, "    mov {rax}, {src_compiled}\n    {operation} {dst_compiled}, {rax}")
        }
    }

    fn generate_comparison<'src, 'a>(
        &mut self,
        function: &'a Function<'src>,
        dst: &Argument,
        lhs: &Argument,
        rhs: &Argument,
        operation: &'a str
    ) -> fmt::Result {
        assert_eq!(*function.argument_data_type(dst), DataType::Bool);

        let rax = NasmRegister::Rax.generate(function.argument_data_type(lhs));

        let lhs_compiled = self.generate_argument(function, lhs)?;
        let rhs_compiled = self.generate_argument(function, rhs)?;
        let dst_compiled = self.generate_argument(function, dst)?;

        writeln!(self.text, "    mov {rax}, {lhs_compiled}\n    cmp {rax}, {rhs_compiled}\n    {operation} {dst_compiled}")
    }

    fn generate_opcode<'src, 'a>(
        &mut self,
        function: &'a Function<'src>,
        opcode: &'a OpCode
    ) -> fmt::Result {
        match opcode {
            OpCode::Mov { dst, src } if dst != src => self.generate_infix(function, dst, src, "mov")?,
            OpCode::Add { dst, src }               => self.generate_infix(function, dst, src, "add")?,
            OpCode::Sub { dst, src }               => self.generate_infix(function, dst, src, "sub")?,
            OpCode::Mul { dst, src } => {
                let rax = NasmRegister::Rax.generate(function.argument_data_type(dst));
                let rbx = NasmRegister::Rbx.generate(function.argument_data_type(dst));

                let src_compiled = self.generate_argument(function, src)?;
                let dst_compiled = self.generate_argument(function, dst)?;

                writeln!(self.text, "    mov {rax}, {dst_compiled}\n    mov {rbx}, {src_compiled}\n    mul {rbx}\n    mov {dst_compiled}, {rax}")?;
            }
            OpCode::Div { dst, src } => {
                let rax = NasmRegister::Rax.generate(function.argument_data_type(dst));
                let rbx = NasmRegister::Rbx.generate(function.argument_data_type(dst));
                let rdx = NasmRegister::Rdx.generate(function.argument_data_type(dst));

                let src_compiled = self.generate_argument(function, src)?;
                let dst_compiled = self.generate_argument(function, dst)?;

                writeln!(self.text, "    xor {rdx}, {rdx}\n    mov {rax}, {dst_compiled}\n    mov {rbx}, {src_compiled}\n    div {rbx}\n    mov {dst_compiled}, {rax}")?;
            }
            OpCode::Mod { dst, src } => {
                let rax = NasmRegister::Rax.generate(function.argument_data_type(dst));
                let rbx = NasmRegister::Rbx.generate(function.argument_data_type(dst));
                let rdx = NasmRegister::Rdx.generate(function.argument_data_type(dst));

                let src_compiled = self.generate_argument(function, src)?;
                let dst_compiled = self.generate_argument(function, dst)?;

                writeln!(self.text, "    xor {rdx}, {rdx}\n    mov {rax}, {dst_compiled}\n    mov {rbx}, {src_compiled}\n    div {rbx}\n    mov {dst_compiled}, {rdx}")?;
            }
            OpCode::Not { dst } => {
                let dst_compiled = self.generate_argument(function, dst)?;

                writeln!(self.text, "    and {dst_compiled}, 0x1\n    xor {dst_compiled}, 0x1")?;
            }
            OpCode::Ref { dst, src } => {
                let rax = NasmRegister::Rax.generate(function.argument_data_type(dst));

                let src_compiled = self.generate_argument(function, src)?;
                let dst_compiled = self.generate_argument(function, dst)?;

                writeln!(self.text, "    lea {rax}, {src_compiled}\n    mov {dst_compiled}, {rax}")?;
            }
            OpCode::Deref { dst, src } => {
                let rax = NasmRegister::Rax.generate(function.argument_data_type(src));
                let rbx = NasmRegister::Rbx.generate(function.argument_data_type(dst));

                let src_compiled = self.generate_argument(function, src)?;
                let dst_compiled = self.generate_argument(function, dst)?;

                writeln!(self.text, "    mov {rax}, {src_compiled}\n    mov {rbx}, [{rax}]\n    mov {dst_compiled}, {rbx}")?;
            }
            OpCode::DerefMov { dst, src } => {
                let rax = NasmRegister::Rax.generate(function.argument_data_type(src));
                let rbx = NasmRegister::Rbx.generate(function.argument_data_type(dst));

                let src_compiled = self.generate_argument(function, src)?;
                let dst_compiled = self.generate_argument(function, dst)?;

                writeln!(self.text, "    mov {rax}, {dst_compiled}\n    mov {rbx}, {src_compiled}\n    mov [{rax}], {rbx}")?;
            }
            OpCode::SetIfEqual { dst, lhs, rhs } => self.generate_comparison(function, dst, lhs, rhs, "sete")?,
            OpCode::SetIfNotEqual { dst, lhs, rhs } => self.generate_comparison(function, dst, lhs, rhs, "setne")?,
            OpCode::SetIfGreater { dst, lhs, rhs } => self.generate_comparison(function, dst, lhs, rhs, "setg")?,
            OpCode::SetIfLess { dst, lhs, rhs } => self.generate_comparison(function, dst, lhs, rhs, "setl")?,
            OpCode::SetIfGreaterOrEqual { dst, lhs, rhs } => self.generate_comparison(function, dst, lhs, rhs, "setge")?,
            OpCode::SetIfLessOrEqual { dst, lhs, rhs } => self.generate_comparison(function, dst, lhs, rhs, "setle")?,
            OpCode::Negate { dst } => {
                let dst_compiled = self.generate_argument(function, dst)?;

                writeln!(self.text, "neg {dst_compiled}")?;
            },
            OpCode::Label { label_id } => writeln!(self.text, ".L{label_id}:")?,
            OpCode::Goto { label_id } => writeln!(self.text, "    jmp .L{label_id}")?,
            OpCode::GotoIfZero {
                condition,
                label_id,
            } => {
                let rax = NasmRegister::Rax.generate(function.argument_data_type(condition));

                let condition_compiled = self.generate_argument(function, condition)?;

                writeln!(self.text, "    mov {rax}, {condition_compiled}\n    test {rax}, {rax}\n    jz .L{label_id}")?;
            }
            OpCode::GotoIfNotZero {
                condition,
                label_id,
            } => {
                let rax = NasmRegister::Rax.generate(function.argument_data_type(condition));

                let condition_compiled = self.generate_argument(function, condition)?;

                writeln!(self.text, "    mov {rax}, {condition_compiled}\n    test {rax}, {rax}\n    jnz .L{label_id}\n")?;
            }
            OpCode::Call {
                dst,
                lhs,
                arguments,
            } => {
                let DataType::Function { return_type, .. } = function.argument_data_type(lhs) else {
                    unreachable!("This should be a function. If there was an error, it should have been caught in the typechecking phase.")
                };

                let lhs_compiled = self.generate_argument(function, lhs)?;

                let dst_compiled = if **return_type == DataType::Void {
                    None
                } else {
                    let compiled = self.generate_argument(function, dst)?;

                    writeln!(self.text, "    push {compiled}")?;

                    Some(compiled)
                };

                for argument in arguments.iter().rev() {
                    let rax = NasmRegister::Rax.generate(function.argument_data_type(argument));

                    let argument_compiled = self.generate_argument(function, argument)?;

                    writeln!(self.text, "    mov {rax}, {argument_compiled}\n    push rax")?;
                }

                if let Argument::Symbol { .. } = lhs {
                    writeln!(self.text, "    call {lhs_compiled}")?;
                } else {
                    let rax = NasmRegister::Rax.generate(function.argument_data_type(lhs));

                    writeln!(self.text, "    mov {rax}, {lhs_compiled}\n    call {rax}")?;
                };

                let argument_stack_size = arguments
                    .iter()
                    .map(|argument| function.argument_data_type(argument).size_aligned())
                    .sum::<usize>();

                writeln!(self.text, "    add rsp, {argument_stack_size}")?;

                if let Some(dst_compiled) = dst_compiled {
                    writeln!(self.text, "pop {dst_compiled}")?;
                }
            },
            _ => {}
        };

        Ok(())
    }

    fn generate_function<'src, 'a>(&mut self, function: &'a Function<'src>) -> fmt::Result {
        writeln!(self.text, "{}:\n    enter {}, 0", function.name, function.stack_size())?;

        for opcode in &function.opcodes {
            self.generate_opcode(function, opcode)?;
        }

        writeln!(self.text, "leave\n    ret")?;

        Ok(())
    }

    fn generate_symbol<'src>(&mut self, (name, bytes): &(String, &'src [u8])) -> fmt::Result {
        let bytes = bytes.iter()
            .map(|byte| byte.to_string())
            .collect::<Vec<String>>()
            .join(", ");

        writeln!(self.text, "{name}: db {bytes}")?;

        Ok(())
    }
}

impl<'src> CodeGenerator<'src> for Nasm {
    fn generate(bytecode: &ByteCode<'src>) -> Result<String, fmt::Error> {
        let mut nasm = Self {
            text: format!("[BITS 64]\nglobal _start\nsection .text\n{PRINT_CODE}{ENTRY_CODE}")
        };

        for function in &bytecode.functions {
            nasm.generate_function(function)?;
        }

        writeln!(nasm.text, "section .data")?;

        for symbol in &bytecode.symbols {
            nasm.generate_symbol(symbol)?;
        }

        Ok(nasm.text)
    }
}
