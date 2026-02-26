#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeId(pub u32);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrimitiveType {
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    F32,
    F64,
    Bool,
    Char,
    Unit,
}

impl PrimitiveType {
    pub fn is_integer(&self) -> bool {
        matches!(
            self,
            PrimitiveType::I8
                | PrimitiveType::I16
                | PrimitiveType::I32
                | PrimitiveType::I64
                | PrimitiveType::U8
                | PrimitiveType::U16
                | PrimitiveType::U32
                | PrimitiveType::U64
        )
    }

    pub fn is_float(&self) -> bool {
        matches!(self, PrimitiveType::F32 | PrimitiveType::F64)
    }

    pub fn is_numeric(&self) -> bool {
        self.is_integer() || self.is_float()
    }

    pub fn is_signed(&self) -> bool {
        matches!(
            self,
            PrimitiveType::I8
                | PrimitiveType::I16
                | PrimitiveType::I32
                | PrimitiveType::I64
                | PrimitiveType::F32
                | PrimitiveType::F64
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeKind {
    Primitive(PrimitiveType),
    String,
    RawPtr,
    MutRawPtr,
    CStr,
    Never,
    Function { params: Vec<TypeId>, ret: TypeId },
    Struct { name: String, fields: Vec<(String, TypeId)> },
    Error,
}

#[derive(Debug)]
pub struct TypeInterner {
    types: Vec<TypeKind>,
}

impl TypeInterner {
    pub fn new() -> Self {
        let mut interner = Self { types: Vec::new() };
        // Pre-intern common types at known indices
        interner.intern(TypeKind::Primitive(PrimitiveType::I8)); // 0
        interner.intern(TypeKind::Primitive(PrimitiveType::I16)); // 1
        interner.intern(TypeKind::Primitive(PrimitiveType::I32)); // 2
        interner.intern(TypeKind::Primitive(PrimitiveType::I64)); // 3
        interner.intern(TypeKind::Primitive(PrimitiveType::U8)); // 4
        interner.intern(TypeKind::Primitive(PrimitiveType::U16)); // 5
        interner.intern(TypeKind::Primitive(PrimitiveType::U32)); // 6
        interner.intern(TypeKind::Primitive(PrimitiveType::U64)); // 7
        interner.intern(TypeKind::Primitive(PrimitiveType::F32)); // 8
        interner.intern(TypeKind::Primitive(PrimitiveType::F64)); // 9
        interner.intern(TypeKind::Primitive(PrimitiveType::Bool)); // 10
        interner.intern(TypeKind::Primitive(PrimitiveType::Char)); // 11
        interner.intern(TypeKind::Primitive(PrimitiveType::Unit)); // 12
        interner.intern(TypeKind::String); // 13
        interner.intern(TypeKind::RawPtr); // 14
        interner.intern(TypeKind::MutRawPtr); // 15
        interner.intern(TypeKind::CStr); // 16
        interner.intern(TypeKind::Never); // 17
        interner.intern(TypeKind::Error); // 18
        interner
    }

    pub fn intern(&mut self, kind: TypeKind) -> TypeId {
        // Check if already interned
        for (i, existing) in self.types.iter().enumerate() {
            if *existing == kind {
                return TypeId(i as u32);
            }
        }
        let id = TypeId(self.types.len() as u32);
        self.types.push(kind);
        id
    }

    pub fn resolve(&self, id: TypeId) -> &TypeKind {
        &self.types[id.0 as usize]
    }

    // Well-known type IDs
    pub fn i8(&self) -> TypeId {
        TypeId(0)
    }
    pub fn i16(&self) -> TypeId {
        TypeId(1)
    }
    pub fn i32(&self) -> TypeId {
        TypeId(2)
    }
    pub fn i64(&self) -> TypeId {
        TypeId(3)
    }
    pub fn u8(&self) -> TypeId {
        TypeId(4)
    }
    pub fn u16(&self) -> TypeId {
        TypeId(5)
    }
    pub fn u32(&self) -> TypeId {
        TypeId(6)
    }
    pub fn u64(&self) -> TypeId {
        TypeId(7)
    }
    pub fn f32(&self) -> TypeId {
        TypeId(8)
    }
    pub fn f64(&self) -> TypeId {
        TypeId(9)
    }
    pub fn bool(&self) -> TypeId {
        TypeId(10)
    }
    pub fn char_type(&self) -> TypeId {
        TypeId(11)
    }
    pub fn unit(&self) -> TypeId {
        TypeId(12)
    }
    pub fn string(&self) -> TypeId {
        TypeId(13)
    }
    pub fn raw_ptr(&self) -> TypeId {
        TypeId(14)
    }
    pub fn mut_raw_ptr(&self) -> TypeId {
        TypeId(15)
    }
    pub fn cstr(&self) -> TypeId {
        TypeId(16)
    }
    pub fn never(&self) -> TypeId {
        TypeId(17)
    }
    pub fn error(&self) -> TypeId {
        TypeId(18)
    }

    pub fn is_struct(&self, id: TypeId) -> bool {
        matches!(self.resolve(id), TypeKind::Struct { .. })
    }

    pub fn iter_types(&self) -> impl Iterator<Item = (TypeId, &TypeKind)> {
        self.types.iter().enumerate().map(|(i, k)| (TypeId(i as u32), k))
    }
}

impl Default for TypeInterner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_interning_dedup() {
        let mut ti = TypeInterner::new();
        let a = ti.intern(TypeKind::Primitive(PrimitiveType::I32));
        let b = ti.intern(TypeKind::Primitive(PrimitiveType::I32));
        assert_eq!(a, b);
        assert_eq!(a, ti.i32());
    }

    #[test]
    fn pre_interned_types() {
        let ti = TypeInterner::new();
        assert_eq!(
            *ti.resolve(ti.i32()),
            TypeKind::Primitive(PrimitiveType::I32)
        );
        assert_eq!(
            *ti.resolve(ti.unit()),
            TypeKind::Primitive(PrimitiveType::Unit)
        );
        assert_eq!(*ti.resolve(ti.string()), TypeKind::String);
    }

    #[test]
    fn all_pre_interned_types() {
        let ti = TypeInterner::new();
        assert_eq!(
            *ti.resolve(ti.i8()),
            TypeKind::Primitive(PrimitiveType::I8)
        );
        assert_eq!(
            *ti.resolve(ti.i16()),
            TypeKind::Primitive(PrimitiveType::I16)
        );
        assert_eq!(
            *ti.resolve(ti.u8()),
            TypeKind::Primitive(PrimitiveType::U8)
        );
        assert_eq!(
            *ti.resolve(ti.u16()),
            TypeKind::Primitive(PrimitiveType::U16)
        );
        assert_eq!(
            *ti.resolve(ti.u32()),
            TypeKind::Primitive(PrimitiveType::U32)
        );
        assert_eq!(
            *ti.resolve(ti.f32()),
            TypeKind::Primitive(PrimitiveType::F32)
        );
        assert_eq!(
            *ti.resolve(ti.f64()),
            TypeKind::Primitive(PrimitiveType::F64)
        );
        assert_eq!(
            *ti.resolve(ti.char_type()),
            TypeKind::Primitive(PrimitiveType::Char)
        );
        assert_eq!(*ti.resolve(ti.raw_ptr()), TypeKind::RawPtr);
        assert_eq!(*ti.resolve(ti.error()), TypeKind::Error);
    }

    #[test]
    fn primitive_type_helpers() {
        assert!(PrimitiveType::I32.is_integer());
        assert!(PrimitiveType::I32.is_numeric());
        assert!(PrimitiveType::I32.is_signed());
        assert!(PrimitiveType::U64.is_integer());
        assert!(!PrimitiveType::U64.is_signed());
        assert!(PrimitiveType::F64.is_float());
        assert!(PrimitiveType::F64.is_numeric());
        assert!(PrimitiveType::F64.is_signed());
        assert!(!PrimitiveType::Bool.is_numeric());
        assert!(!PrimitiveType::Char.is_numeric());
    }
}
