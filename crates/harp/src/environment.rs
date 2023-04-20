//
// environment.rs
//
// Copyright (C) 2022 Posit Software, PBC. All rights reserved.
//
//

use std::ops::Deref;

use libR_sys::*;

use crate::exec::RFunction;
use crate::exec::RFunctionExt;
use crate::object::RObject;
use crate::symbol::RSymbol;
use crate::utils::Sxpinfo;
use crate::utils::r_assert_type;
use crate::utils::r_inherits;
use crate::utils::r_is_altrep;
use crate::utils::r_is_null;
use crate::utils::r_typeof;
use crate::vector::CharacterVector;
use crate::vector::Vector;
use crate::vector::collapse;

#[derive(Debug, Eq, PartialEq)]
pub enum BindingKind {
    Regular,
    Active,
    Promise(bool),
}

#[derive(Eq, PartialEq)]
pub struct BindingExtraAltrep {
    data1: SEXP,
    data2: SEXP,
}

#[derive(Eq, PartialEq)]
pub enum BindingExtra {
    None,
    Altrep(BindingExtraAltrep)
}

#[derive(Eq, PartialEq)]
pub struct Binding {
    pub name: RSymbol,
    pub value: SEXP,
    pub kind: BindingKind,
    pub extra: BindingExtra,
}

impl Binding {
    pub fn new(env: SEXP, frame: SEXP) -> Self {
        unsafe {
            let name = RSymbol::new(TAG(frame));

            let info = Sxpinfo::interpret(&frame);

            if info.is_immediate() {
                // force the immediate bindings before we can safely call CAR()
                Rf_findVarInFrame(env, *name);
            }
            let value = CAR(frame);

            let kind = if info.is_active() {
                BindingKind::Active
            } else {
                match r_typeof(value) {
                    PROMSXP => BindingKind::Promise(PRVALUE(value) != R_UnboundValue),
                    _       => BindingKind::Regular
                }
            };

            let extra = if r_is_altrep(value) {
                BindingExtra::Altrep(BindingExtraAltrep{
                    data1: R_altrep_data1(value),
                    data2: R_altrep_data2(value)
                })
            } else {
                BindingExtra::None
            };

            Self {
                name,
                value,
                kind,
                extra
            }
        }

    }

    pub fn is_hidden(&self) -> bool {
        String::from(self.name).starts_with(".")
    }

}

pub fn has_children(value: SEXP) -> bool {
    if RObject::view(value).is_s4() {
        unsafe {
            let names = RFunction::new("methods", ".slotNames").add(value).call().unwrap();
            let names = CharacterVector::new_unchecked(names);
            names.len() > 0
        }
    } else {
        match r_typeof(value) {
            VECSXP   => unsafe { XLENGTH(value) != 0 },
            EXPRSXP  => unsafe { XLENGTH(value) != 0 },
            LISTSXP  => true,
            ENVSXP   => true,
            _        => false
        }
    }
}

pub fn is_simple_vector(value: SEXP) -> bool {
    unsafe {
        let class = Rf_getAttrib(value, R_ClassSymbol);

        match r_typeof(value) {
            LGLSXP | REALSXP | CPLXSXP | STRSXP | RAWSXP => r_is_null(class),
            INTSXP  => r_is_null(class) || r_inherits(value, "factor"),

            _       => false
        }
    }
}

pub fn first_class(value: SEXP) -> Option<String> {
    unsafe {
        let classes = Rf_getAttrib(value, R_ClassSymbol);
        if r_is_null(classes) {
            None
        } else {
            let classes = CharacterVector::new_unchecked(classes);
            Some(classes.get_unchecked(0).unwrap())
        }
    }
}

pub fn all_classes(value: SEXP) -> String {
    unsafe {
        let classes = Rf_getAttrib(value, R_ClassSymbol);
        collapse(classes, "/", 0, "").unwrap().result
    }
}

pub fn pairlist_size(mut pairlist: SEXP) -> Result<isize, crate::error::Error> {
    let mut n = 0;
    unsafe {
        while pairlist != R_NilValue {
            r_assert_type(pairlist, &[LISTSXP])?;

            pairlist = CDR(pairlist);
            n = n + 1;
        }
    }
    Ok(n)
}

pub fn vec_type(value: SEXP) -> String {
    match r_typeof(value) {
        INTSXP  => unsafe {
            if r_inherits(value, "factor") {
                let levels = Rf_getAttrib(value, R_LevelsSymbol);
                format!("fct({})", XLENGTH(levels))
            } else {
                String::from("int")
            }
        },
        REALSXP => String::from("dbl"),
        LGLSXP  => String::from("lgl"),
        STRSXP  => String::from("str"),
        RAWSXP  => String::from("raw"),
        CPLXSXP => String::from("cplx"),

        // TODO: this should not happen
        _       => String::from("???")
    }
}

pub fn vec_shape(value: SEXP) -> String {
    unsafe {
        let dim = RObject::new(Rf_getAttrib(value, R_DimSymbol));

        if r_is_null(*dim) {
            if XLENGTH(value) == 1 {
                String::from("")
            } else {
                format!(" [{}]", Rf_xlength(value))
            }
        } else {
            format!(" [{}]", collapse(*dim, ",", 0, "").unwrap().result)
        }
    }
}

pub fn altrep_class(object: SEXP) -> String {
    let serialized_klass = unsafe{
        ATTRIB(ALTREP_CLASS(object))
    };

    let klass = RSymbol::new(unsafe{CAR(serialized_klass)});
    let pkg = RSymbol::new(unsafe{CADR(serialized_klass)});

    format!("{}::{}", pkg, klass)
}

pub struct Environment {
    env: RObject,
}

impl Deref for Environment {
    type Target = SEXP;
    fn deref(&self) -> &Self::Target {
        &self.env.sexp
    }
}

pub struct HashedEnvironmentIter<'a> {
    env: &'a Environment,

    hashtab: SEXP,
    hashtab_index: R_xlen_t,
    frame: SEXP
}

impl<'a> HashedEnvironmentIter<'a> {
    pub fn new(env: &'a Environment) -> Self {
        unsafe {
            let hashtab = HASHTAB(**env);
            let hashtab_len = XLENGTH(hashtab);
            let mut hashtab_index = 0;
            let mut frame = R_NilValue;

            // look for the first non null frame
            loop {
                let f = VECTOR_ELT(hashtab, hashtab_index);
                if f != R_NilValue {
                    frame = f;
                    break;
                }

                hashtab_index = hashtab_index + 1;
                if hashtab_index == hashtab_len {
                    break;
                }
            }

            Self {
                env,
                hashtab,
                hashtab_index,
                frame
            }

        }
    }
}

impl<'a> Iterator for HashedEnvironmentIter<'a> {
    type Item = Binding;

    fn next(&mut self) -> Option<Self::Item> {

        unsafe {
            if self.frame == R_NilValue {
                return None;
            }

            // grab the next Binding
            let binding = Binding::new(*self.env.env, self.frame);

            // and advance to next binding
            self.frame = CDR(self.frame);

            if self.frame == R_NilValue {
                // end of frame: move to the next non empty frame
                let hashtab_len = XLENGTH(self.hashtab);
                loop {
                    // move to the next frame
                    self.hashtab_index = self.hashtab_index + 1;

                    // end of iteration
                    if self.hashtab_index == hashtab_len {
                        self.frame = R_NilValue;
                        break;
                    }

                    // skip empty frames
                    self.frame = VECTOR_ELT(self.hashtab, self.hashtab_index);
                    if self.frame != R_NilValue {
                        break;
                    }
                }
            }

            Some(binding)

        }
    }
}

pub struct NonHashedEnvironmentIter<'a> {
    env: &'a Environment,

    frame: SEXP
}

impl<'a> NonHashedEnvironmentIter<'a> {
    pub fn new(env: &'a Environment) -> Self {
        unsafe {
            Self {
                env,
                frame: FRAME(**env),
            }
        }
    }
}

impl<'a> Iterator for NonHashedEnvironmentIter<'a> {
    type Item = Binding;

    fn next(&mut self) -> Option<Self::Item> {
        unsafe {
            if self.frame == R_NilValue {
                None
            } else {
                let binding = Binding::new(*self.env.env, self.frame);
                self.frame = CDR(self.frame);
                Some(binding)
            }
        }
    }
}

pub enum EnvironmentIter<'a> {
    Hashed(HashedEnvironmentIter<'a>),
    NonHashed(NonHashedEnvironmentIter<'a>)
}

impl<'a> EnvironmentIter<'a> {
    pub fn new(env: &'a Environment) -> Self {

        unsafe {
            let hashtab = HASHTAB(**env);
            if hashtab == R_NilValue {
                EnvironmentIter::NonHashed(NonHashedEnvironmentIter::new(env))
            } else {
                EnvironmentIter::Hashed(HashedEnvironmentIter::new(env))
            }
        }
    }
}

impl<'a> Iterator for EnvironmentIter<'a> {
    type Item = Binding;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            EnvironmentIter::Hashed(iter) => iter.next(),
            EnvironmentIter::NonHashed(iter) => iter.next()
        }
    }
}

impl Environment {
    pub fn new(value: SEXP) -> Self {
        Self {
            env: unsafe{ RObject::new(value) }
        }
    }

    pub fn iter(&self) -> EnvironmentIter {
        EnvironmentIter::new(&self)
    }
}

#[cfg(test)]
mod tests {
    use libR_sys::*;

    use crate::r_symbol;
    use crate::r_test;

    use super::*;

    unsafe fn test_environment_iter_impl(hash: bool) {
        let test_env = RFunction::new("base", "new.env")
            .param("parent", R_EmptyEnv)
            .param("hash", RObject::from(hash))
            .call()
            .unwrap();

        let sym = r_symbol!("a");
        Rf_defineVar(sym, Rf_ScalarInteger(42), test_env.sexp);

        let sym = r_symbol!("b");
        Rf_defineVar(sym, Rf_ScalarInteger(43), test_env.sexp);

        let sym = r_symbol!("c");
        Rf_defineVar(sym, Rf_ScalarInteger(44), test_env.sexp);

        let env = Environment::new(*test_env);
        assert_eq!(env.iter().count(), 3);
    }

    #[test]
    #[allow(non_snake_case)]
    fn test_environment_iter() { r_test! {
        test_environment_iter_impl(true);
        test_environment_iter_impl(false);
    }}

}