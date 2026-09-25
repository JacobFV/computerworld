//! Questions the lowerer asks of a stretch of syntax before lowering it.

use std::collections::BTreeSet;

use oxc_ast::ast;
use oxc_ast_visit::{walk, Visit};
use oxc_semantic::ScopeFlags;

/// Which of `names` a statement reads from inside a nested function (so a closure
/// may keep the binding), and which it assigns anywhere. Shadowing is ignored: the
/// answers err towards "yes".
pub(crate) struct NameUse<'n> {
    names: &'n [String],
    depth: u32,
    pub captured: BTreeSet<String>,
    pub assigned: BTreeSet<String>,
}

impl<'n> NameUse<'n> {
    pub fn of_statement<'a>(names: &'n [String], s: &ast::Statement<'a>) -> NameUse<'n> {
        let mut v = NameUse {
            names,
            depth: 0,
            captured: BTreeSet::new(),
            assigned: BTreeSet::new(),
        };
        v.visit_statement(s);
        v
    }

    fn named(&self, n: &str) -> bool {
        self.names.iter().any(|x| x == n)
    }
}

impl<'a> Visit<'a> for NameUse<'_> {
    fn visit_function(&mut self, it: &ast::Function<'a>, flags: ScopeFlags) {
        self.depth += 1;
        walk::walk_function(self, it, flags);
        self.depth -= 1;
    }

    fn visit_arrow_function_expression(&mut self, it: &ast::ArrowFunctionExpression<'a>) {
        self.depth += 1;
        walk::walk_arrow_function_expression(self, it);
        self.depth -= 1;
    }

    fn visit_identifier_reference(&mut self, it: &ast::IdentifierReference<'a>) {
        if self.depth > 0 && self.named(it.name.as_str()) {
            self.captured.insert(it.name.to_string());
        }
    }

    fn visit_simple_assignment_target(&mut self, it: &ast::SimpleAssignmentTarget<'a>) {
        if let ast::SimpleAssignmentTarget::AssignmentTargetIdentifier(id) = it {
            if self.named(id.name.as_str()) {
                self.assigned.insert(id.name.to_string());
            }
        }
        walk::walk_simple_assignment_target(self, it);
    }
}
