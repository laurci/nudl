#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeId(pub u32);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrimitiveType {
    I32,
    I64,
    U64,
    Bool,
    Unit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeKind {
    Primitive(PrimitiveType),
    String,
    RawPtr,
    Function { params: Vec<TypeId>, ret: TypeId },
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
        interner.intern(TypeKind::Primitive(PrimitiveType::I32)); // 0
        interner.intern(TypeKind::Primitive(PrimitiveType::I64)); // 1
        interner.intern(TypeKind::Primitive(PrimitiveType::U64)); // 2
        interner.intern(TypeKind::Primitive(PrimitiveType::Bool)); // 3
        interner.intern(TypeKind::Primitive(PrimitiveType::Unit)); // 4
        interner.intern(TypeKind::String); // 5
        interner.intern(TypeKind::RawPtr); // 6
        interner.intern(TypeKind::Error); // 7
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
    pub fn i32(&self) -> TypeId {
        TypeId(0)
    }
    pub fn i64(&self) -> TypeId {
        TypeId(1)
    }
    pub fn u64(&self) -> TypeId {
        TypeId(2)
    }
    pub fn bool(&self) -> TypeId {
        TypeId(3)
    }
    pub fn unit(&self) -> TypeId {
        TypeId(4)
    }
    pub fn string(&self) -> TypeId {
        TypeId(5)
    }
    pub fn raw_ptr(&self) -> TypeId {
        TypeId(6)
    }
    pub fn error(&self) -> TypeId {
        TypeId(7)
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
}
