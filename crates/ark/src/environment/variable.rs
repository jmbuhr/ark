//
// variable.rs
//
// Copyright (C) 2023 by Posit Software, PBC
//
//

use harp::environment::Binding;
use harp::environment::BindingKind;
use harp::environment::BindingType;
use harp::environment::BindingValue;
use harp::environment::env_bindings;
use harp::exec::RFunction;
use harp::exec::RFunctionExt;
use harp::object::RObject;
use harp::r_symbol;
use harp::symbol::RSymbol;
use harp::utils::r_assert_type;
use harp::utils::r_inherits;
use harp::utils::r_typeof;
use harp::vector::CharacterVector;
use harp::vector::Vector;
use libR_sys::*;
use serde::Deserialize;
use serde::Serialize;

/// Represents the supported kinds of variable values.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ValueKind {
    /// A length-1 logical vector
    Boolean,

    /// A raw byte array
    Bytes,

    /// A collection of unnamed values; usually a vector
    Collection,

    /// Empty/missing values such as NULL, NA, or missing
    Empty,

    /// A function, method, closure, or other callable object
    Function,

    /// Named lists of values, such as lists and (hashed) environments
    Map,

    /// A number, such as an integer or floating-point value
    Number,

    /// A value of an unknown or unspecified type
    Other,

    /// A character string
    String,

    /// A table, dataframe, 2D matrix, or other two-dimensional data structure
    Table,
}

/// Represents the serialized form of an environment variable.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EnvironmentVariable {
    /** The access key; not displayed to the user, but used to form path accessors */
    pub access_key: String,

    /** The environment variable's name, formatted for display */
    pub display_name: String,

    /** The environment variable's value, formatted for display */
    pub display_value: String,

    /** The environment variable's type, formatted for display */
    pub display_type: String,

    /** Extended type information */
    pub type_info: String,

    /** The environment variable's value kind (string, number, etc.) */
    pub kind: ValueKind,

    /** The number of elements in the variable's value, if applicable */
    pub length: usize,

    /** The size of the variable's value, in bytes */
    pub size: usize,

    /** True if the variable contains other variables */
    pub has_children: bool,

    /** True if the 'value' field was truncated to fit in the message */
    pub is_truncated: bool,
}

impl EnvironmentVariable {
    /**
     * Create a new EnvironmentVariable from a Binding
     */
    pub fn new(binding: &Binding) -> Self {
        let display_name = binding.name.to_string();

        let BindingValue {
            display_value,
            is_truncated,
        } = binding.get_value();
        let BindingType {
            display_type,
            type_info,
        } = binding.get_type();

        let kind = match binding.kind {
            BindingKind::Active => ValueKind::Other,
            BindingKind::Promise(false) => ValueKind::Other,
            BindingKind::Promise(true) => Self::variable_kind(unsafe { PRVALUE(binding.value) }),
            BindingKind::Regular => Self::variable_kind(binding.value),
        };
        let has_children = binding.has_children();

        Self {
            access_key: display_name.clone(),
            display_name,
            display_value,
            display_type,
            type_info,
            kind,
            length: 0,
            size: 0,
            has_children,
            is_truncated,
        }
    }

    /**
     * Create a new EnvironmentVariable from an R object
     */
    fn from(access_key: String, display_name: String, x: SEXP) -> Self {
        let BindingValue{display_value, is_truncated} = BindingValue::from(x);
        let BindingType{display_type, type_info} = BindingType::from(x);
        let has_children = harp::environment::has_children(x);

        Self {
            access_key,
            display_name,
            display_value,
            display_type,
            type_info,
            kind: Self::variable_kind(x),
            length: 0,
            size: 0,
            has_children,
            is_truncated
        }
    }

    fn variable_kind(x: SEXP) -> ValueKind {
        match r_typeof(x) {
            CLOSXP => ValueKind::Function,
            ENVSXP => ValueKind::Map,
            VECSXP => {
                if unsafe{ r_inherits(x, "data.frame") } {
                    ValueKind::Table
                } else {
                    unsafe {
                        let names = Rf_getAttrib(x, R_NamesSymbol) ;
                        if names == R_NilValue {
                            ValueKind::Collection
                        } else {
                            ValueKind::Map
                        }
                    }
                }
            },

            LGLSXP  => ValueKind::Collection,
            INTSXP  => ValueKind::Collection,
            REALSXP => ValueKind::Collection,
            CPLXSXP => ValueKind::Collection,
            STRSXP  => ValueKind::Collection,
            RAWSXP  => ValueKind::Collection,

            _       => ValueKind::Other
        }
    }

    pub fn inspect(env: RObject, path: &Vec<String>) -> Result<Vec<Self>, harp::error::Error> {
        let object = unsafe {
            Self::resolve_object_from_path(env, &path)?
        };

        // expansions specific to the type
        let mut out = match r_typeof(*object) {
            VECSXP  => Self::inspect_list(*object),
            LISTSXP => Self::inspect_pairlist(*object),
            ENVSXP  => Self::inspect_environment(*object),
            _       => Ok(vec![])
        }? ;

        // attributes
        unsafe {
            let attributes = ATTRIB(*object);
            if attributes != R_NilValue {
                let mut attributes = Self::inspect_pairlist(attributes)?;

                for i in 0..attributes.len() {
                    let var = attributes.get_mut(i).unwrap();
                    var.access_key   = format!("@{}", var.display_name);
                    var.display_name = format!("attr(\"{}\")", var.display_name);
                }

                out.append(&mut attributes);
            }
        }

        Ok(out)
    }

    unsafe fn resolve_object_from_path(mut object: RObject, path: &Vec<String>) -> Result<RObject, harp::error::Error> {
        for path_element in path {

            if path_element.starts_with("@") {
                let (_, name) = path_element.split_at(1);

                let mut attributes = ATTRIB(*object);
                while attributes != R_NilValue {
                    if String::from(RSymbol::new(TAG(attributes))) == name {
                        object = RObject::view(CAR(attributes));

                        break;
                    }
                    attributes = CDR(attributes);
                }
            } else {
                let rtype = r_typeof(*object);
                object = match rtype {
                    ENVSXP => {
                        // TODO: active bindings and promises can't be inspected at the moment,
                        //       so we can safely assume we can call Rf_findVarInFrame()
                        //       without forcing them, but it might be something we want to relax in the future
                        //       e.g. if we want to be able to expand a promise to show its code and/or env
                        RObject::view(unsafe { Rf_findVarInFrame(*object, r_symbol!(path_element)) } )
                    },
                    VECSXP => {
                        let index = path_element.parse::<isize>().unwrap();
                        RObject::view(VECTOR_ELT(*object, index))
                    },

                    LISTSXP => {
                        let mut pairlist = *object;
                        let index = path_element.parse::<isize>().unwrap();
                        for _i in 0..index {
                            pairlist = CDR(pairlist);
                        }
                        RObject::view(CAR(pairlist))
                    }

                    _ => return Err( harp::error::Error::UnexpectedType(rtype, vec![ENVSXP, VECSXP, LISTSXP]))
                };
            }
        }

        Ok(object)
    }

    fn inspect_list(value: SEXP) -> Result<Vec<Self>, harp::error::Error> {
        let mut out : Vec<Self> = vec![];
        let n = unsafe { XLENGTH(value) };

        let names = unsafe {
            CharacterVector::new_unchecked(RFunction::from(".ps.environment.listDisplayNames").add(value).call()?)
        };

        for i in 0..n {
            out.push(Self::from(
                i.to_string(),
                names.get_unchecked(i).unwrap(),
                unsafe{ VECTOR_ELT(value, i)}
            ));
        }

        Ok(out)
    }

    fn inspect_pairlist(value: SEXP) -> Result<Vec<Self>, harp::error::Error> {
        let mut out : Vec<Self> = vec![];

        let mut pairlist = value;
        unsafe {
            let mut i = 0;
            while pairlist != R_NilValue {

                r_assert_type(pairlist, &[LISTSXP])?;

                let tag = TAG(pairlist);
                let display_name = if tag == R_NilValue {
                    format!("[[{}]]", i + 1)
                } else {
                    String::from(RSymbol::new(tag))
                };

                out.push(Self::from(i.to_string(), display_name, CAR(pairlist)));

                pairlist = CDR(pairlist);
                i = i + 1;
            }
        }

        Ok(out)
    }

    fn inspect_environment(value: SEXP) -> Result<Vec<Self>, harp::error::Error> {
        let mut out : Vec<Self> = vec![];

        for binding in &env_bindings(value) {
            out.push(Self::new(binding));
        }

        out.sort_by(|a, b| {
            a.display_name.cmp(&b.display_name)
        });

        Ok(out)
    }

}
