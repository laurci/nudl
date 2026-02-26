use std::collections::HashMap;

/// A stack of local-variable scopes.
///
/// `push_scope()` opens a new scope; `pop_scope()` discards every binding that
/// was introduced in that scope, restoring the previous state.  Lookups walk
/// from the innermost scope outward so inner bindings shadow outer ones.
#[derive(Debug, Clone)]
pub struct ScopedLocals<V: Clone> {
    /// Each entry is a map of names introduced in that scope.
    scopes: Vec<HashMap<String, V>>,
}

impl<V: Clone> ScopedLocals<V> {
    pub fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
        }
    }

    /// Open a new (empty) scope.
    pub fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    /// Close the innermost scope, discarding its bindings.
    pub fn pop_scope(&mut self) {
        debug_assert!(self.scopes.len() > 1, "cannot pop the root scope");
        self.scopes.pop();
    }

    /// Insert a binding into the current (innermost) scope.
    pub fn insert(&mut self, name: String, value: V) {
        self.scopes.last_mut().unwrap().insert(name, value);
    }

    /// Look up a name, searching from the innermost scope outward.
    pub fn get(&self, name: &str) -> Option<&V> {
        for scope in self.scopes.iter().rev() {
            if let Some(v) = scope.get(name) {
                return Some(v);
            }
        }
        None
    }

    /// Snapshot all visible bindings into a flat map.
    /// Used by the lowerer's loop copyback which needs a flat view.
    pub fn flatten(&self) -> HashMap<String, V> {
        let mut flat = HashMap::new();
        for scope in &self.scopes {
            for (k, v) in scope {
                flat.insert(k.clone(), v.clone());
            }
        }
        flat
    }

    /// Update an existing binding in the scope where it was originally defined.
    /// Returns `true` if the binding was found and updated, `false` otherwise.
    pub fn update(&mut self, name: &str, value: V) -> bool {
        for scope in self.scopes.iter_mut().rev() {
            if scope.contains_key(name) {
                scope.insert(name.to_string(), value);
                return true;
            }
        }
        false
    }
}
