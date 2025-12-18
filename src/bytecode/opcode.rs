//! Bytecode opcodes for the Quicksilver VM
//!
//! This module defines the instruction set used by the interpreter.
//! Instructions are designed to be compact and efficient for interpretation.

/// Bytecode opcodes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Opcode {
    // ========== Stack Operations ==========
    /// No operation
    Nop = 0x00,
    /// Pop the top value from the stack
    Pop = 0x01,
    /// Duplicate the top value on the stack
    Dup = 0x02,
    /// Swap the top two values on the stack
    Swap = 0x03,

    // ========== Constants ==========
    /// Load a constant from the constant pool
    /// Operands: constant_index (u16)
    Constant = 0x10,
    /// Push undefined
    Undefined = 0x11,
    /// Push null
    Null = 0x12,
    /// Push true
    True = 0x13,
    /// Push false
    False = 0x14,

    // ========== Variables ==========
    /// Load a local variable
    /// Operands: local_index (u8)
    GetLocal = 0x20,
    /// Store to a local variable
    /// Operands: local_index (u8)
    SetLocal = 0x21,
    /// Load a global variable
    /// Operands: name_index (u16)
    GetGlobal = 0x22,
    /// Store to a global variable
    /// Operands: name_index (u16)
    SetGlobal = 0x23,
    /// Define a global variable
    /// Operands: name_index (u16)
    DefineGlobal = 0x24,
    /// Load an upvalue (closure variable)
    /// Operands: upvalue_index (u16)
    GetUpvalue = 0x25,
    /// Store to an upvalue
    /// Operands: upvalue_index (u16)
    SetUpvalue = 0x26,
    /// Close an upvalue (move to heap)
    /// Operands: upvalue_index (u16)
    CloseUpvalue = 0x27,

    // ========== Registers ==========
    /// Load from register
    /// Operands: register (u8)
    LoadReg = 0x28,
    /// Store to register
    /// Operands: register (u8)
    StoreReg = 0x29,

    // ========== Properties ==========
    /// Get a property by name
    /// Operands: name_index (u16)
    GetProperty = 0x30,
    /// Set a property by name
    /// Operands: name_index (u16)
    SetProperty = 0x31,
    /// Define a property
    /// Operands: name_index (u16)
    DefineProperty = 0x32,
    /// Get an element by index (from stack)
    GetElement = 0x33,
    /// Set an element by index (from stack)
    SetElement = 0x34,
    /// Get a private field by name
    /// Operands: name_index (u16)
    GetPrivateField = 0x35,
    /// Set a private field by name
    /// Operands: name_index (u16)
    SetPrivateField = 0x36,
    /// Define a private field
    /// Operands: name_index (u16)
    DefinePrivateField = 0x37,

    // ========== Arithmetic ==========
    /// Add two values
    Add = 0x40,
    /// Subtract two values
    Sub = 0x41,
    /// Multiply two values
    Mul = 0x42,
    /// Divide two values
    Div = 0x43,
    /// Modulo two values
    Mod = 0x44,
    /// Exponentiation
    Pow = 0x45,
    /// Negate a value
    Neg = 0x46,
    /// Increment
    Increment = 0x47,
    /// Decrement
    Decrement = 0x48,

    // ========== Bitwise ==========
    /// Bitwise NOT
    BitwiseNot = 0x50,
    /// Bitwise AND
    BitwiseAnd = 0x51,
    /// Bitwise OR
    BitwiseOr = 0x52,
    /// Bitwise XOR
    BitwiseXor = 0x53,
    /// Left shift
    Shl = 0x54,
    /// Right shift (signed)
    Shr = 0x55,
    /// Right shift (unsigned)
    UShr = 0x56,

    // ========== Comparison ==========
    /// Equal (==)
    Eq = 0x60,
    /// Not equal (!=)
    Ne = 0x61,
    /// Strict equal (===)
    StrictEq = 0x62,
    /// Strict not equal (!==)
    StrictNe = 0x63,
    /// Less than (<)
    Lt = 0x64,
    /// Less than or equal (<=)
    Le = 0x65,
    /// Greater than (>)
    Gt = 0x66,
    /// Greater than or equal (>=)
    Ge = 0x67,

    // ========== Logical ==========
    /// Logical NOT
    Not = 0x70,

    // ========== Type Operations ==========
    /// typeof operator
    Typeof = 0x80,
    /// void operator
    Void = 0x81,
    /// delete operator
    Delete = 0x82,
    /// in operator
    In = 0x83,
    /// instanceof operator
    Instanceof = 0x84,

    // ========== Control Flow ==========
    /// Unconditional jump
    /// Operands: offset (i16)
    Jump = 0x90,
    /// Jump if top of stack is falsy
    /// Operands: offset (i16)
    JumpIfFalse = 0x91,
    /// Jump if top of stack is truthy
    /// Operands: offset (i16)
    JumpIfTrue = 0x92,
    /// Jump if top of stack is null/undefined
    /// Operands: offset (i16)
    JumpIfNull = 0x93,
    /// Jump if top of stack is not null/undefined
    /// Operands: offset (i16)
    JumpIfNotNull = 0x94,

    // ========== Functions ==========
    /// Call a function
    /// Operands: arg_count (u8)
    Call = 0xA0,
    /// Return from function with value
    Return = 0xA1,
    /// Return undefined from function
    ReturnUndefined = 0xA2,
    /// new operator
    /// Operands: arg_count (u8)
    New = 0xA3,
    /// Tail call a function (optimized for recursion)
    /// Operands: arg_count (u8)
    TailCall = 0xA7,
    /// Create a function from bytecode
    /// Operands: function_index (u16)
    CreateFunction = 0xA4,
    /// Create a closure
    /// Operands: function_index (u16)
    CreateClosure = 0xA5,
    /// Call a method on an object
    /// Operands: name_index (u16), arg_count (u8)
    /// Stack: [receiver, args...] -> [result]
    CallMethod = 0xA6,

    // ========== Objects ==========
    /// Create an array
    /// Operands: element_count (u8)
    CreateArray = 0xB0,
    /// Create an object
    /// Operands: property_count (u8)
    CreateObject = 0xB1,
    /// Create a class
    /// Operands: class_index (u16)
    CreateClass = 0xB2,
    /// Push this
    This = 0xB3,
    /// Push super
    Super = 0xB4,
    /// new.target
    NewTarget = 0xB5,
    /// Set superclass for class inheritance
    /// Stack: [class, super_class] -> [class]
    SetSuperClass = 0xB6,
    /// Call super constructor
    /// Operands: arg_count (u8)
    /// Stack: [args...] -> [result]
    SuperCall = 0xB7,
    /// Get property from super
    /// Operands: name_index (u16)
    SuperGet = 0xB8,

    // ========== Iteration ==========
    /// Get iterator from value
    GetIterator = 0xC0,
    /// Call iterator.next()
    IteratorNext = 0xC1,
    /// Check if iterator is done
    IteratorDone = 0xC2,
    /// Get value from iterator result
    IteratorValue = 0xC3,

    // ========== Exception Handling ==========
    /// Enter try block
    /// Operands: catch_offset (u16)
    EnterTry = 0xD0,
    /// Leave try block
    LeaveTry = 0xD1,
    /// Throw exception
    Throw = 0xD2,

    // ========== With Statement ==========
    /// Enter with block
    /// Operands: (none)
    EnterWith = 0xE0,
    /// Leave with block
    LeaveWith = 0xE1,

    // ========== Spread/Rest ==========
    /// Spread operator
    Spread = 0xF0,
    /// Rest parameter
    RestParam = 0xF1,

    // ========== Async/Generator ==========
    /// Yield value
    Yield = 0xF8,
    /// Await promise
    Await = 0xF9,

    // ========== Modules ==========
    /// Load a module by specifier
    /// Operands: specifier_index (u16)
    LoadModule = 0xFA,
    /// Export a value with a name
    /// Operands: name_index (u16)
    ExportValue = 0xFB,
    /// Re-export all from a module (module on stack)
    ExportAll = 0xFC,
    /// Dynamic import - import(source) returns Promise<Module>
    DynamicImport = 0xFD,
}

impl Opcode {
    /// Convert a byte to an opcode
    pub fn from_u8(byte: u8) -> Option<Opcode> {
        match byte {
            0x00 => Some(Opcode::Nop),
            0x01 => Some(Opcode::Pop),
            0x02 => Some(Opcode::Dup),
            0x03 => Some(Opcode::Swap),

            0x10 => Some(Opcode::Constant),
            0x11 => Some(Opcode::Undefined),
            0x12 => Some(Opcode::Null),
            0x13 => Some(Opcode::True),
            0x14 => Some(Opcode::False),

            0x20 => Some(Opcode::GetLocal),
            0x21 => Some(Opcode::SetLocal),
            0x22 => Some(Opcode::GetGlobal),
            0x23 => Some(Opcode::SetGlobal),
            0x24 => Some(Opcode::DefineGlobal),
            0x25 => Some(Opcode::GetUpvalue),
            0x26 => Some(Opcode::SetUpvalue),
            0x27 => Some(Opcode::CloseUpvalue),
            0x28 => Some(Opcode::LoadReg),
            0x29 => Some(Opcode::StoreReg),

            0x30 => Some(Opcode::GetProperty),
            0x31 => Some(Opcode::SetProperty),
            0x32 => Some(Opcode::DefineProperty),
            0x33 => Some(Opcode::GetElement),
            0x34 => Some(Opcode::SetElement),
            0x35 => Some(Opcode::GetPrivateField),
            0x36 => Some(Opcode::SetPrivateField),
            0x37 => Some(Opcode::DefinePrivateField),

            0x40 => Some(Opcode::Add),
            0x41 => Some(Opcode::Sub),
            0x42 => Some(Opcode::Mul),
            0x43 => Some(Opcode::Div),
            0x44 => Some(Opcode::Mod),
            0x45 => Some(Opcode::Pow),
            0x46 => Some(Opcode::Neg),
            0x47 => Some(Opcode::Increment),
            0x48 => Some(Opcode::Decrement),

            0x50 => Some(Opcode::BitwiseNot),
            0x51 => Some(Opcode::BitwiseAnd),
            0x52 => Some(Opcode::BitwiseOr),
            0x53 => Some(Opcode::BitwiseXor),
            0x54 => Some(Opcode::Shl),
            0x55 => Some(Opcode::Shr),
            0x56 => Some(Opcode::UShr),

            0x60 => Some(Opcode::Eq),
            0x61 => Some(Opcode::Ne),
            0x62 => Some(Opcode::StrictEq),
            0x63 => Some(Opcode::StrictNe),
            0x64 => Some(Opcode::Lt),
            0x65 => Some(Opcode::Le),
            0x66 => Some(Opcode::Gt),
            0x67 => Some(Opcode::Ge),

            0x70 => Some(Opcode::Not),

            0x80 => Some(Opcode::Typeof),
            0x81 => Some(Opcode::Void),
            0x82 => Some(Opcode::Delete),
            0x83 => Some(Opcode::In),
            0x84 => Some(Opcode::Instanceof),

            0x90 => Some(Opcode::Jump),
            0x91 => Some(Opcode::JumpIfFalse),
            0x92 => Some(Opcode::JumpIfTrue),
            0x93 => Some(Opcode::JumpIfNull),
            0x94 => Some(Opcode::JumpIfNotNull),

            0xA0 => Some(Opcode::Call),
            0xA1 => Some(Opcode::Return),
            0xA2 => Some(Opcode::ReturnUndefined),
            0xA3 => Some(Opcode::New),
            0xA4 => Some(Opcode::CreateFunction),
            0xA5 => Some(Opcode::CreateClosure),
            0xA6 => Some(Opcode::CallMethod),
            0xA7 => Some(Opcode::TailCall),

            0xB0 => Some(Opcode::CreateArray),
            0xB1 => Some(Opcode::CreateObject),
            0xB2 => Some(Opcode::CreateClass),
            0xB3 => Some(Opcode::This),
            0xB4 => Some(Opcode::Super),
            0xB5 => Some(Opcode::NewTarget),
            0xB6 => Some(Opcode::SetSuperClass),
            0xB7 => Some(Opcode::SuperCall),
            0xB8 => Some(Opcode::SuperGet),

            0xC0 => Some(Opcode::GetIterator),
            0xC1 => Some(Opcode::IteratorNext),
            0xC2 => Some(Opcode::IteratorDone),
            0xC3 => Some(Opcode::IteratorValue),

            0xD0 => Some(Opcode::EnterTry),
            0xD1 => Some(Opcode::LeaveTry),
            0xD2 => Some(Opcode::Throw),

            0xE0 => Some(Opcode::EnterWith),
            0xE1 => Some(Opcode::LeaveWith),

            0xF0 => Some(Opcode::Spread),
            0xF1 => Some(Opcode::RestParam),

            0xF8 => Some(Opcode::Yield),
            0xF9 => Some(Opcode::Await),

            0xFA => Some(Opcode::LoadModule),
            0xFB => Some(Opcode::ExportValue),
            0xFC => Some(Opcode::ExportAll),
            0xFD => Some(Opcode::DynamicImport),

            _ => None,
        }
    }

    /// Get the size of the instruction including operands
    pub fn instruction_size(&self) -> usize {
        match self {
            // No operands
            Opcode::Nop
            | Opcode::Pop
            | Opcode::Dup
            | Opcode::Swap
            | Opcode::Undefined
            | Opcode::Null
            | Opcode::True
            | Opcode::False
            | Opcode::Add
            | Opcode::Sub
            | Opcode::Mul
            | Opcode::Div
            | Opcode::Mod
            | Opcode::Pow
            | Opcode::Neg
            | Opcode::Increment
            | Opcode::Decrement
            | Opcode::BitwiseNot
            | Opcode::BitwiseAnd
            | Opcode::BitwiseOr
            | Opcode::BitwiseXor
            | Opcode::Shl
            | Opcode::Shr
            | Opcode::UShr
            | Opcode::Eq
            | Opcode::Ne
            | Opcode::StrictEq
            | Opcode::StrictNe
            | Opcode::Lt
            | Opcode::Le
            | Opcode::Gt
            | Opcode::Ge
            | Opcode::Not
            | Opcode::Typeof
            | Opcode::Void
            | Opcode::Delete
            | Opcode::In
            | Opcode::Instanceof
            | Opcode::Return
            | Opcode::ReturnUndefined
            | Opcode::This
            | Opcode::Super
            | Opcode::NewTarget
            | Opcode::GetElement
            | Opcode::SetElement
            | Opcode::GetIterator
            | Opcode::IteratorNext
            | Opcode::IteratorDone
            | Opcode::IteratorValue
            | Opcode::LeaveTry
            | Opcode::Throw
            | Opcode::LeaveWith
            | Opcode::Spread
            | Opcode::RestParam
            | Opcode::Yield
            | Opcode::Await
            | Opcode::ExportAll
            | Opcode::DynamicImport => 1,

            // 1-byte operand
            Opcode::GetLocal
            | Opcode::SetLocal
            | Opcode::LoadReg
            | Opcode::StoreReg
            | Opcode::Call
            | Opcode::TailCall
            | Opcode::New
            | Opcode::CreateArray
            | Opcode::CreateObject => 2,

            // 2-byte operand
            Opcode::Constant
            | Opcode::GetGlobal
            | Opcode::SetGlobal
            | Opcode::DefineGlobal
            | Opcode::GetUpvalue
            | Opcode::SetUpvalue
            | Opcode::CloseUpvalue
            | Opcode::GetProperty
            | Opcode::SetProperty
            | Opcode::DefineProperty
            | Opcode::GetPrivateField
            | Opcode::SetPrivateField
            | Opcode::DefinePrivateField
            | Opcode::Jump
            | Opcode::JumpIfFalse
            | Opcode::JumpIfTrue
            | Opcode::JumpIfNull
            | Opcode::JumpIfNotNull
            | Opcode::CreateFunction
            | Opcode::CreateClosure
            | Opcode::CreateClass
            | Opcode::EnterTry
            | Opcode::EnterWith
            | Opcode::LoadModule
            | Opcode::ExportValue
            | Opcode::SuperGet => 3,

            // 3-byte operand (u16 + u8)
            Opcode::CallMethod => 4,

            // No operands for SetSuperClass
            Opcode::SetSuperClass => 1,

            // 1-byte operand (u8) for SuperCall
            Opcode::SuperCall => 2,
        }
    }
}
