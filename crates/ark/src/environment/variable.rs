//
// variable.rs
//
// Copyright (C) 2023 by Posit Software, PBC
//
//

use harp::environment::Binding;
use harp::environment::BindingType;
use harp::environment::BindingValue;
use harp::exec::RFunction;
use harp::exec::RFunctionExt;
use harp::object::RObject;
use harp::r_symbol;
use harp::utils::r_typeof;
use harp::vector::CharacterVector;
use harp::vector::Vector;
use libR_sys::Rf_findVarInFrame;
use libR_sys::VECSXP;
use libR_sys::VECTOR_ELT;
use libR_sys::XLENGTH;
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

        let BindingValue{display_value, is_truncated} = binding.get_value();
        let BindingType{display_type, type_info} = binding.get_type();

        let kind = ValueKind::String;
        let has_children = binding.has_children();

        Self {
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

    pub fn inspect(env: RObject, path: Vec<String>) -> Vec<Self> {
        // for now only lists can be expanded
        let list = unsafe {
            RFunction::from(".ps.environment_extract")
                .param("env", env)
                .param("path", CharacterVector::create(path).cast())
                .call()
                .unwrap()
        };

        let mut out : Vec<Self> = vec![];
        let n = unsafe { XLENGTH(*list) };

        let names = unsafe{
            CharacterVector::new_unchecked(RFunction::from(".ps.list_display_names").add(*list).call().unwrap())
        };

        for i in 0..n {
            let x = RObject::view(unsafe{ VECTOR_ELT(*list, i)});
            let display_name = names.get_unchecked(i).unwrap();
            let BindingValue{display_value, is_truncated} = BindingValue::from(*x);
            let BindingType{display_type, type_info} = BindingType::from(*x);
            let has_children = r_typeof(*x) == VECSXP;

            out.push(Self {
                display_name,
                display_value,
                display_type,
                type_info,
                kind: ValueKind::Other,
                length: 0,
                size: 0,
                has_children,
                is_truncated
            });
        }

        out
    }

}
