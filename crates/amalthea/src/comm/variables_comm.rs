/*---------------------------------------------------------------------------------------------
 *  Copyright (C) 2023 Posit Software, PBC. All rights reserved.
 *--------------------------------------------------------------------------------------------*/

//
// AUTO-GENERATED from variables.json; do not edit.
//

use serde::Deserialize;
use serde::Serialize;

/// An inspected variable.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct InspectedVariable {
	/// The children of the inspected variable.
	pub children: Vec<Variable>,

	/// The total number of children. This may be greater than the number of
	/// children in the 'children' array if the array is truncated.
	pub length: i64
}

/// An object formatted for copying to the clipboard.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct FormattedVariable {
	/// The format returned, as a MIME type; matches the MIME type of the
	/// format named in the request.
	pub format: String,

	/// The formatted content of the variable.
	pub content: String
}

/// A single variable in the runtime.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct Variable {
	/// A key that uniquely identifies the variable within the runtime and can
	/// be used to access the variable in `inspect` requests
	pub access_key: String,

	/// The name of the variable, formatted for display
	pub display_name: String,

	/// A string representation of the variable's value, formatted for display
	/// and possibly truncated
	pub display_value: String,

	/// The variable's type, formatted for display
	pub display_type: String,

	/// Extended information about the variable's type
	pub type_info: String,

	/// The size of the variable's value in bytes
	pub size: i64,

	/// The kind of value the variable represents, such as 'string' or
	/// 'number'
	pub kind: String,

	/// The number of elements in the variable, if it is a collection
	pub length: i64,

	/// Whether the variable has child variables
	pub has_children: bool,

	/// True if there is a viewer available for this variable (i.e. the
	/// runtime can handle a 'view' request for this variable)
	pub has_viewer: bool,

	/// True the 'value' field is a truncated representation of the variable's
	/// value
	pub is_truncated: bool
}

/// Possible values for Kind in Variable
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub enum VariableKind {
	#[serde(rename = "boolean")]
	Boolean,

	#[serde(rename = "bytes")]
	Bytes,

	#[serde(rename = "collection")]
	Collection,

	#[serde(rename = "empty")]
	Empty,

	#[serde(rename = "function")]
	Function,

	#[serde(rename = "map")]
	Map,

	#[serde(rename = "number")]
	Number,

	#[serde(rename = "other")]
	Other,

	#[serde(rename = "string")]
	String,

	#[serde(rename = "table")]
	Table
}

/// Parameters for the Clear method.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct ClearParams {
	/// Whether to clear hidden objects in addition to normal variables
	pub include_hidden_objects: bool,
}

/// Parameters for the Delete method.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct DeleteParams {
	/// The names of the variables to delete.
	pub names: Vec<String>,
}

/// Parameters for the Inspect method.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct InspectParams {
	/// The path to the variable to inspect, as an array of access keys.
	pub path: Vec<String>,
}

/// Parameters for the ClipboardFormat method.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct ClipboardFormatParams {
	/// The path to the variable to format, as an array of access keys.
	pub path: Vec<String>,

	/// The requested format for the variable, as a MIME type
	pub format: String,
}

/// Parameters for the View method.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct ViewParams {
	/// The path to the variable to view, as an array of access keys.
	pub path: Vec<String>,
}

/// Parameters for the Update method.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct UpdateParams {
	/// An array of variables that have been newly assigned.
	pub assigned: Vec<Variable>,

	/// An array of variable names that have been removed.
	pub removed: Vec<String>,
}

/// Parameters for the Refresh method.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct RefreshParams {
	/// An array listing all the variables in the current session.
	pub variables: Vec<Variable>,
}

/**
 * RPC request types for the variables comm
 */
#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "method", content = "params")]
pub enum VariablesRpcRequest {
	/// List all variables
	///
	/// Returns a list of all the variables in the current session.
	#[serde(rename = "list")]
	List,

	/// Clear all variables
	///
	/// Clears (deletes) all variables in the current session.
	#[serde(rename = "clear")]
	Clear(ClearParams),

	/// Deletes a set of named variables
	///
	/// Deletes the named variables from the current session.
	#[serde(rename = "delete")]
	Delete(DeleteParams),

	/// Inspect a variable
	///
	/// Returns the children of a variable, as an array of variables.
	#[serde(rename = "inspect")]
	Inspect(InspectParams),

	/// Format for clipboard
	///
	/// Requests a formatted representation of a variable for copying to the
	/// clipboard.
	#[serde(rename = "clipboard_format")]
	ClipboardFormat(ClipboardFormatParams),

	/// Request a viewer for a variable
	///
	/// Request that the runtime open a data viewer to display the data in a
	/// variable.
	#[serde(rename = "view")]
	View(ViewParams),

}

/**
 * RPC Reply types for the variables comm
 */
#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "method", content = "result")]
pub enum VariablesRpcReply {
	/// A list of variables in the session.
	ListReply(Vec<Variable>),

	/// A list of variables in the session remaining after deletion; usually
	/// empty.
	ClearReply(Vec<Variable>),

	/// The names of the variables that were successfully deleted.
	DeleteReply(Vec<String>),

	/// An inspected variable.
	InspectReply(InspectedVariable),

	/// An object formatted for copying to the clipboard.
	ClipboardFormatReply(FormattedVariable),

}

/**
 * Front-end events for the variables comm
 */
#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "method", content = "params")]
pub enum VariablesEvent {
	/// Updates the variables in the current session.
	#[serde(rename = "update")]
	Update(UpdateParams),

	/// Replace all variables in the current session with the variables from
	/// the backend.
	#[serde(rename = "refresh")]
	Refresh(RefreshParams),

}

