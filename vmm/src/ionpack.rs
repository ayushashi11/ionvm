//! IonPack format - ZIP-based container for IonVM modules
//! 
//! IonPack files (.ionpack) are ZIP archives with a specific structure:
//! - META-INF/MANIFEST.ion - Package metadata
//! - classes/ - Compiled IonVM bytecode files (.ionc)
//! - lib/ - Native FFI libraries (.so, .dll, .dylib)
//! - resources/ - Static resources
//! - src/ - Optional source files

use crate::bytecode_binary::{serialize_function, serialize_functions, deserialize_functions_auto, resolve_function_references, BytecodeError};
use crate::value::Function;
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Write, Seek};
use std::path::{Path, PathBuf};
use zip::{ZipWriter, ZipArchive, write::FileOptions, CompressionMethod};

/// Magic identifier for IonPack files
pub const IONPACK_MAGIC: &str = "ionpack";

/// Current IonPack format version
pub const IONPACK_VERSION: &str = "1.0";

/// Error type for IonPack operations
#[derive(Debug)]
pub enum IonPackError {
    IoError(io::Error),
    ZipError(zip::result::ZipError),
    BytecodeError(BytecodeError),
    InvalidFormat(String),
    MissingMetadata,
    InvalidManifest(String),
    ClassNotFound(String),
    FunctionNotFound(String),
    MainClassNotSpecified,
    MainFunctionNotFound,
    DependencyError(String),
    FFIError(String),
}

impl From<io::Error> for IonPackError {
    fn from(err: io::Error) -> Self {
        IonPackError::IoError(err)
    }
}

impl From<zip::result::ZipError> for IonPackError {
    fn from(err: zip::result::ZipError) -> Self {
        IonPackError::ZipError(err)
    }
}

impl From<BytecodeError> for IonPackError {
    fn from(err: BytecodeError) -> Self {
        IonPackError::BytecodeError(err)
    }
}

impl std::fmt::Display for IonPackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IonPackError::IoError(e) => write!(f, "IO error: {}", e),
            IonPackError::ZipError(e) => write!(f, "ZIP error: {}", e),
            IonPackError::BytecodeError(e) => write!(f, "Bytecode error: {}", e),
            IonPackError::InvalidFormat(s) => write!(f, "Invalid format: {}", s),
            IonPackError::MissingMetadata => write!(f, "Missing metadata"),
            IonPackError::InvalidManifest(s) => write!(f, "Invalid manifest: {}", s),
            IonPackError::ClassNotFound(s) => write!(f, "Class not found: {}", s),
            IonPackError::FunctionNotFound(s) => write!(f, "Function not found: {}", s),
            IonPackError::MainClassNotSpecified => write!(f, "Main class not specified"),
            IonPackError::MainFunctionNotFound => write!(f, "Main function not found"),
            IonPackError::DependencyError(s) => write!(f, "Dependency error: {}", s),
            IonPackError::FFIError(s) => write!(f, "FFI error: {}", s),
        }
    }
}

impl std::error::Error for IonPackError {}

/// IonPack manifest metadata
#[derive(Debug, Clone)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    pub main_class: Option<String>,
    pub entry_point: Option<String>,
    pub description: Option<String>,
    pub author: Option<String>,
    pub dependencies: Vec<String>,
    pub ffi_libraries: Vec<String>,
    pub exports: Vec<String>,
    pub ionpack_version: String,
}

impl Manifest {
    pub fn new(name: String, version: String) -> Self {
        Self {
            name,
            version,
            main_class: None,
            entry_point: None,
            description: None,
            author: None,
            dependencies: Vec::new(),
            ffi_libraries: Vec::new(),
            exports: Vec::new(),
            ionpack_version: IONPACK_VERSION.to_string(),
        }
    }

    /// Serialize manifest to MANIFEST.ion format
    pub fn to_string(&self) -> String {
        let mut result = String::new();
        result.push_str(&format!("IonPack-Version: {}\n", self.ionpack_version));
        result.push_str(&format!("Name: {}\n", self.name));
        result.push_str(&format!("Version: {}\n", self.version));
        
        if let Some(ref main) = self.main_class {
            result.push_str(&format!("Main-Class: {}\n", main));
        }
        
        if let Some(ref entry) = self.entry_point {
            result.push_str(&format!("Entry-Point: {}\n", entry));
        }
        
        if let Some(ref desc) = self.description {
            result.push_str(&format!("Description: {}\n", desc));
        }
        
        if let Some(ref author) = self.author {
            result.push_str(&format!("Author: {}\n", author));
        }
        
        if !self.dependencies.is_empty() {
            result.push_str(&format!("Dependencies: {}\n", self.dependencies.join(", ")));
        }
        
        if !self.ffi_libraries.is_empty() {
            result.push_str(&format!("FFI-Libraries: {}\n", self.ffi_libraries.join(", ")));
        }
        
        if !self.exports.is_empty() {
            result.push_str(&format!("Exports: {}\n", self.exports.join(", ")));
        }
        
        result
    }

    /// Parse manifest from MANIFEST.ion format
    pub fn from_string(content: &str) -> Result<Self, IonPackError> {
        let mut manifest = Manifest::new("unnamed".to_string(), "1.0".to_string());
        
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            
            if let Some((key, value)) = line.split_once(':') {
                let key = key.trim();
                let value = value.trim();
                
                match key {
                    "IonPack-Version" => manifest.ionpack_version = value.to_string(),
                    "Name" => manifest.name = value.to_string(),
                    "Version" => manifest.version = value.to_string(),
                    "Main-Class" => manifest.main_class = Some(value.to_string()),
                    "Entry-Point" => manifest.entry_point = Some(value.to_string()),
                    "Description" => manifest.description = Some(value.to_string()),
                    "Author" => manifest.author = Some(value.to_string()),
                    "Dependencies" => {
                        manifest.dependencies = value.split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                    },
                    "FFI-Libraries" => {
                        manifest.ffi_libraries = value.split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                    },
                    "Exports" => {
                        manifest.exports = value.split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                    },
                    _ => {} // Unknown field, ignore
                }
            } else {
                return Err(IonPackError::InvalidManifest(
                    format!("Invalid manifest line: {}", line)
                ));
            }
        }
        
        Ok(manifest)
    }
}

/// IonPack builder for creating packages
pub struct IonPackBuilder {
    manifest: Manifest,
    classes: HashMap<String, Vec<u8>>, // class_name -> bytecode
    libraries: HashMap<String, Vec<u8>>, // lib_name -> binary data
    resources: HashMap<String, Vec<u8>>, // resource_path -> data
    sources: HashMap<String, String>, // source_path -> source code
}

impl IonPackBuilder {
    pub fn new(name: String, version: String) -> Self {
        Self {
            manifest: Manifest::new(name, version),
            classes: HashMap::new(),
            libraries: HashMap::new(),
            resources: HashMap::new(),
            sources: HashMap::new(),
        }
    }

    pub fn main_class(mut self, main_class: String) -> Self {
        self.manifest.main_class = Some(main_class);
        self
    }

    pub fn entry_point(mut self, entry_point: String) -> Self {
        self.manifest.entry_point = Some(entry_point);
        self
    }

    pub fn description(mut self, description: String) -> Self {
        self.manifest.description = Some(description);
        self
    }

    pub fn author(mut self, author: String) -> Self {
        self.manifest.author = Some(author);
        self
    }

    pub fn dependency(mut self, dep: String) -> Self {
        self.manifest.dependencies.push(dep);
        self
    }

    pub fn export(mut self, export: String) -> Self {
        self.manifest.exports.push(export);
        self
    }

    /// Add a compiled class (function) to the package
    pub fn add_class(&mut self, name: &str, function: &Function) -> Result<(), IonPackError> {
        let mut buffer = Vec::new();
        serialize_function(function, &mut buffer)?;
        self.classes.insert(name.to_string(), buffer);
        Ok(())
    }

    /// Add multiple functions as a single class (multi-function format)
    pub fn add_multi_function_class(&mut self, name: &str, functions: &[Function]) -> Result<(), IonPackError> {
        let mut buffer = Vec::new();
        serialize_functions(functions, &mut buffer)?;
        self.classes.insert(name.to_string(), buffer);
        Ok(())
    }

    /// Add an FFI library to the package
    pub fn add_library<P: AsRef<Path>>(&mut self, name: &str, path: P) -> Result<(), IonPackError> {
        let mut file = File::open(path)?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)?;
        self.libraries.insert(name.to_string(), buffer);
        self.manifest.ffi_libraries.push(name.to_string());
        Ok(())
    }

    /// Add a resource file to the package
    pub fn add_resource<P: AsRef<Path>>(&mut self, path: &str, file_path: P) -> Result<(), IonPackError> {
        let mut file = File::open(file_path)?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)?;
        self.resources.insert(path.to_string(), buffer);
        Ok(())
    }

    /// Add source code to the package
    pub fn add_source(&mut self, path: &str, source: String) {
        self.sources.insert(path.to_string(), source);
    }

    /// Build the IonPack file
    pub fn build<W: Write + Seek>(self, writer: W) -> Result<(), IonPackError> {
        let mut zip = ZipWriter::new(writer);
        let options = FileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .unix_permissions(0o644);

        // Write manifest
        zip.start_file("META-INF/MANIFEST.ion", options)?;
        zip.write_all(self.manifest.to_string().as_bytes())?;

        // Write classes
        for (name, bytecode) in self.classes {
            let path = format!("classes/{}.ionc", name);
            zip.start_file(&path, options)?;
            zip.write_all(&bytecode)?;
        }

        // Write libraries
        for (name, data) in self.libraries {
            let path = format!("lib/{}", name);
            zip.start_file(&path, options)?;
            zip.write_all(&data)?;
        }

        // Write resources
        for (path, data) in self.resources {
            let resource_path = format!("resources/{}", path);
            zip.start_file(&resource_path, options)?;
            zip.write_all(&data)?;
        }

        // Write sources
        for (path, source) in self.sources {
            let source_path = format!("src/{}", path);
            zip.start_file(&source_path, options)?;
            zip.write_all(source.as_bytes())?;
        }

        zip.finish()?;
        Ok(())
    }
}

/// IonPack reader for loading packages
pub struct IonPackReader<R: Read + Seek> {
    archive: ZipArchive<R>,
    manifest: Manifest,
}

impl<R: Read + Seek> IonPackReader<R> {
    pub fn new(reader: R) -> Result<Self, IonPackError> {
        let mut archive = ZipArchive::new(reader)?;
        
        // Read manifest
        let manifest = {
            let mut manifest_file = archive.by_name("META-INF/MANIFEST.ion")?;
            let mut content = String::new();
            manifest_file.read_to_string(&mut content)?;
            Manifest::from_string(&content)?
        };

        Ok(Self { archive, manifest })
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// List all classes in the package
    pub fn list_classes(&mut self) -> Result<Vec<String>, IonPackError> {
        let mut classes = Vec::new();
        for i in 0..self.archive.len() {
            let file = self.archive.by_index(i)?;
            let name = file.name();
            if name.starts_with("classes/") && name.ends_with(".ionc") {
                let class_name = &name[8..name.len()-5]; // Remove "classes/" and ".ionc"
                classes.push(class_name.to_string());
            }
        }
        Ok(classes)
    }

    /// Read a specific class bytecode
    pub fn read_class(&mut self, name: &str) -> Result<Vec<u8>, IonPackError> {
        let path = format!("classes/{}.ionc", name);
        let mut file = self.archive.by_name(&path)?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)?;
        Ok(buffer)
    }

    /// List all FFI libraries
    pub fn list_libraries(&mut self) -> Result<Vec<String>, IonPackError> {
        let mut libraries = Vec::new();
        for i in 0..self.archive.len() {
            let file = self.archive.by_index(i)?;
            let name = file.name();
            if name.starts_with("lib/") {
                let lib_name = &name[4..]; // Remove "lib/"
                libraries.push(lib_name.to_string());
            }
        }
        Ok(libraries)
    }

    /// Extract an FFI library to a temporary location
    pub fn extract_library(&mut self, name: &str, target_dir: &Path) -> Result<PathBuf, IonPackError> {
        let path = format!("lib/{}", name);
        let mut file = self.archive.by_name(&path)?;
        
        let target_path = target_dir.join(name);
        if let Some(parent) = target_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        
        let mut target_file = File::create(&target_path)?;
        io::copy(&mut file, &mut target_file)?;
        
        Ok(target_path)
    }

    /// Read a resource file
    pub fn read_resource(&mut self, path: &str) -> Result<Vec<u8>, IonPackError> {
        let resource_path = format!("resources/{}", path);
        let mut file = self.archive.by_name(&resource_path)?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)?;
        Ok(buffer)
    }

    /// Read source code
    pub fn read_source(&mut self, path: &str) -> Result<String, IonPackError> {
        let source_path = format!("src/{}", path);
        let mut file = self.archive.by_name(&source_path)?;
        let mut content = String::new();
        file.read_to_string(&mut content)?;
        Ok(content)
    }

    /// Load and deserialize a function from a class
    pub fn load_function(&mut self, class_name: &str) -> Result<Function, IonPackError> {
        let functions = self.load_functions(class_name)?;
        if functions.is_empty() {
            return Err(IonPackError::ClassNotFound(class_name.to_string()));
        }
        // Return the first function for backward compatibility
        Ok(functions.into_iter().next().unwrap())
    }

    /// Load and deserialize all functions from a class (supports both single and multi-function formats)
    pub fn load_functions(&mut self, class_name: &str) -> Result<Vec<Function>, IonPackError> {
        let bytecode = self.read_class(class_name)?;
        
        use std::io::Cursor;
        
        let cursor = Cursor::new(bytecode);
        deserialize_functions_auto(cursor)
            .map_err(|e| IonPackError::BytecodeError(e))
    }

    /// Load a specific function by name from a class
    pub fn load_function_by_name(&mut self, class_name: &str, function_name: &str) -> Result<Function, IonPackError> {
        let functions = self.load_functions(class_name)?;
        for function in functions {
            if let Some(ref name) = function.name {
                if name == function_name {
                    return Ok(function);
                }
            }
        }
        Err(IonPackError::FunctionNotFound(function_name.to_string()))
    }

    /// Load a function and resolve any embedded function references
    pub fn load_function_with_registry(&mut self, function_name: &str, function_registry: &HashMap<String, Function>) -> Result<Function, IonPackError> {
        let bytecode = self.read_class(function_name)?;
        
        use crate::bytecode_binary::deserialize_function_with_registry;
        use std::io::Cursor;
        
        let mut cursor = Cursor::new(bytecode);
        deserialize_function_with_registry(&mut cursor, function_registry)
            .map_err(|e| IonPackError::BytecodeError(e))
    }

    /// Load all functions from the IonPack into a registry (supports multi-function classes)
    pub fn load_all_functions(&mut self) -> Result<HashMap<String, Function>, IonPackError> {
        let class_names = self.list_classes()?;
        let mut functions = HashMap::new();
        let mut class_functions_map = HashMap::new();
        
        // First pass: load all functions from all classes and organize by class
        for class_name in &class_names {
            let class_functions = self.load_functions(class_name)?;
            class_functions_map.insert(class_name.clone(), class_functions.clone());
            
            for function in class_functions {
                if let Some(ref function_name) = function.name {
                    // Use function name as key if available
                    functions.insert(function_name.clone(), function.clone());
                    // Also add with class_name:function_name for unique identification
                    functions.insert(format!("{}:{}", class_name, function_name), function);
                } else {
                    // Fallback to class name for unnamed functions
                    functions.insert(class_name.clone(), function);
                }
            }
        }
        
        // Second pass: resolve function references with class-aware resolution
        let mut resolved_functions = HashMap::new();
        for (class_name, class_functions) in &class_functions_map {
            for function in class_functions.clone() {
                // Create a class-local function registry for intra-class references
                let mut class_local_registry = HashMap::new();
                for f in class_functions {
                    if let Some(ref fname) = f.name {
                        class_local_registry.insert(fname.clone(), f.clone());
                    }
                }
                // Include global functions as well
                for (name, f) in &functions {
                    class_local_registry.insert(name.clone(), f.clone());
                }
                
                if let Some(ref function_name) = function.name {
                    //resolved_functions.insert(function_name.clone(), function.clone());
                    resolved_functions.insert(format!("{}:{}", class_name, function_name), function);
                } else {
                    resolved_functions.insert(class_name.clone(), function);
                }
            }
        }
        
        let resolv_clone = resolved_functions.clone();
        for name in resolv_clone.keys() {
            println!("found: {}", name);
        }
        for (name, function) in &mut resolved_functions {
            // Resolve any references within the function
            println!("Resolving function references for: {}", name);
            resolve_function_references(function, &resolv_clone);
        }
        
        Ok(resolved_functions)
    }

    /// Get the main function for CLI execution
    /// 
    /// CLI execution follows this resolution order:
    /// 1. If Entry-Point is specified in manifest, use that exact function
    /// 2. If Main-Class is specified in manifest, load that class and find main function
    /// 3. For multi-function classes, find the first function with arity 0
    /// 4. For single-function classes, use that function
    /// 5. If no Main-Class is specified, return an error
    /// 6. Resolve function references within the class
    pub fn get_main_function(&mut self) -> Result<Function, IonPackError> {
        if let Ok(fns) = self.load_all_functions() {    
            // Fall back to Main-Class behavior
            let main_class = self.manifest.main_class.clone()
                .ok_or(IonPackError::MainClassNotSpecified)?;

            // First check if Entry-Point is specified
            if let Some(ref entry_point) = self.manifest.entry_point {
                if let Some(function) = fns.get(format!("{}:{}", main_class, entry_point).as_str()) {
                    return Ok(function.clone());
                } else {
                    return Err(IonPackError::FunctionNotFound(entry_point.clone()));
                }
            }
            
            if let Some(function) = fns.get(format!("{}:main", main_class).as_str()) {
                // If it's a multi-function class, find the first function with arity 0
                if function.arity == 0 {
                    return Ok(function.clone());
                } 
            } else {
                return Err(IonPackError::ClassNotFound(main_class));
            }
        }
        
        Err(IonPackError::MainFunctionNotFound)
    }

    /// Setup FFI libraries for execution
    /// Extracts FFI libraries to a temporary directory and returns their paths
    pub fn setup_ffi_libraries(&mut self, temp_dir: &Path) -> Result<Vec<PathBuf>, IonPackError> {
        let mut extracted_libs = Vec::new();
        
        for lib_name in &self.manifest.ffi_libraries.clone() {
            let lib_path = self.extract_library(lib_name, temp_dir)?;
            extracted_libs.push(lib_path);
        }
        
        Ok(extracted_libs)
    }

    /// List all available resources in the package
    pub fn list_resources(&mut self) -> Result<Vec<String>, IonPackError> {
        let mut resources = Vec::new();
        for i in 0..self.archive.len() {
            let file = self.archive.by_index(i)?;
            let name = file.name();
            if name.starts_with("resources/") {
                let resource_name = &name[10..]; // Remove "resources/"
                resources.push(resource_name.to_string());
            }
        }
        Ok(resources)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Function;
    use crate::vm::Instruction;
    use crate::value::{Value, Primitive};
    use std::io::{Cursor, SeekFrom};

    #[test]
    fn test_manifest_serialization() {
        let mut manifest = Manifest::new("test-package".to_string(), "1.0.0".to_string());
        manifest.main_class = Some("Main".to_string());
        manifest.description = Some("Test package".to_string());
        manifest.dependencies.push("std".to_string());

        let serialized = manifest.to_string();
        let deserialized = Manifest::from_string(&serialized).unwrap();

        assert_eq!(manifest.name, deserialized.name);
        assert_eq!(manifest.version, deserialized.version);
        assert_eq!(manifest.main_class, deserialized.main_class);
        assert_eq!(manifest.description, deserialized.description);
        assert_eq!(manifest.dependencies, deserialized.dependencies);
    }

    #[test]
    fn test_ionpack_creation() {
        let mut builder = IonPackBuilder::new("test-app".to_string(), "1.0".to_string())
            .main_class("Main".to_string())
            .description("Test application".to_string());

        // Add a simple function
        let function = Function::new_bytecode(
            Some("main".to_string()),
            0,
            0,  // extra_regs - this simple function doesn't need extra registers
            vec![
                Instruction::LoadConst(0, Value::Primitive(Primitive::Number(42.0))),
                Instruction::Return(0),
            ]
        );

        builder.add_class("Main", &function).unwrap();
        builder.add_source("Main.ion", "function main() { return 42; }".to_string());

        let mut buffer = Cursor::new(Vec::new());
        builder.build(&mut buffer).unwrap();

        // Test reading back
        buffer.seek(SeekFrom::Start(0)).unwrap();
        let mut reader = IonPackReader::new(buffer).unwrap();

        assert_eq!(reader.manifest().name, "test-app");
        assert_eq!(reader.manifest().main_class, Some("Main".to_string()));

        let classes = reader.list_classes().unwrap();
        assert!(classes.contains(&"Main".to_string()));

        let bytecode = reader.read_class("Main").unwrap();
        assert!(!bytecode.is_empty());

        let source = reader.read_source("Main.ion").unwrap();
        assert_eq!(source, "function main() { return 42; }");
    }
}
