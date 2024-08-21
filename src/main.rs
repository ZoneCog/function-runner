use std::{
    fs::File,
    io::{stdin, BufReader, Read},
    path::PathBuf,
};

use anyhow::{anyhow, Result};

use clap::{Parser, ValueEnum};
use function_runner::engine::{run, FunctionRunParams, ProfileOpts}; // Adjust the import based on actual module path

use is_terminal::IsTerminal;

use bluejay_parser::{
    ast::{
        definition::{DefaultContext, DefinitionDocument, SchemaDefinition},
        executable::ExecutableDocument,
        Parse,
    },
    Error,
};

use bluejay_core::definition::ObjectTypeDefinition;

use bluejay_core::definition::SchemaDefinition as CoreSchemaDefinition;

const PROFILE_DEFAULT_INTERVAL: u32 = 500_000; // every 5us

/// Supported input flavors
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
enum Codec {
    /// JSON input, must be valid JSON
    Json,
    /// Raw input, no validation, passed as-is
    Raw,
    /// JSON input, will be converted to MessagePack, must be valid JSON
    JsonToMessagepack,
}

/// Simple Function runner which takes JSON as a convenience.
#[derive(Parser, Debug)]
#[clap(version)]
#[command(arg_required_else_help = true)]
struct Opts {
    /// Path to wasm/wat Function
    #[clap(short, long, default_value = "function.wasm")]
    function: PathBuf,

    /// Path to json file containing Function input; if omitted, stdin is used
    #[clap(short, long)]
    input: Option<PathBuf>,

    /// Name of the export to invoke.
    #[clap(short, long, default_value = "_start")]
    export: String,

    /// Log the run result as a JSON object
    #[clap(short, long)]
    json: bool,

    /// Enable profiling. This will make your Function run slower.
    /// The resulting profile can be used in speedscope (https://www.speedscope.app/)
    /// Specifying --profile-* argument will also enable profiling.
    #[clap(short, long)]
    profile: bool,

    /// Where to save the profile information. Defaults to ./{wasm-filename}.perf.
    #[clap(long)]
    profile_out: Option<PathBuf>,

    /// How many samples per seconds. Defaults to 500_000 (every 5us).
    #[clap(long)]
    profile_frequency: Option<u32>,

    #[clap(short = 'c', long, value_enum, default_value = "json")]
    codec: Codec,

    // Also takes in schema string, CLI can generate this via 'generate schema'
    /// Path to json file containing Function input; if omitted, stdin is used
    #[clap(short = 's', long, default_value = "schema.graphql")]
    schema_path: Option<PathBuf>,

    // Also takes in schema string, CLI can generate this via 'generate schema'
    /// Path to json file containing Function input; if omitted, stdin is used
    #[clap(short = 'q', long, default_value = "input.graphql")]
    query_path: Option<PathBuf>,
}

impl Opts {
    pub fn profile_opts(&self) -> Option<ProfileOpts> {
        if !self.profile && self.profile_out.is_none() && self.profile_frequency.is_none() {
            return None;
        }

        let interval = self.profile_frequency.unwrap_or(PROFILE_DEFAULT_INTERVAL);
        let out = self
            .profile_out
            .clone()
            .unwrap_or_else(|| self.default_profile_out());

        Some(ProfileOpts { interval, out })
    }

    fn default_profile_out(&self) -> PathBuf {
        let mut path = PathBuf::new();

        path.set_file_name(
            self.function
                .file_name()
                .unwrap_or(std::ffi::OsStr::new("function")),
        );
        path.set_extension("perf");

        path
    }

    // Reads the schema file and returns its contents as a String.
    pub fn read_schema_to_string(&self) -> Result<String> {
        match &self.schema_path {
            Some(schema_path) => {
                let mut file = File::open(schema_path)
                    .map_err(|e| anyhow!("Couldn't open schema file {:?}: {}", schema_path, e))?;
                let mut contents = String::new();
                file.read_to_string(&mut contents)
                    .map_err(|e| anyhow!("Couldn't read schema file {:?}: {}", schema_path, e))?;
                Ok(contents)
            }
            None => Err(anyhow!("Schema file path is not provided")),
        }
    }

    pub fn read_query_to_string(&self) -> Result<String> {
        match &self.query_path {
            Some(query_path) => {
                let mut file = File::open(query_path)
                    .map_err(|e| anyhow!("Couldn't open schema file {:?}: {}", query_path, e))?;
                let mut contents = String::new();
                file.read_to_string(&mut contents)
                    .map_err(|e| anyhow!("Couldn't read schema file {:?}: {}", query_path, e))?;
                Ok(contents)
            }
            None => Err(anyhow!("Schema file path is not provided")),
        }
    }
}

fn create_definition_document(schema_string: &str) -> Result<DefinitionDocument, Vec<Error>> {
    let result: Result<DefinitionDocument, _> = DefinitionDocument::parse(schema_string);
    result
}

fn create_schema_definition(definition_document: DefinitionDocument, query: &str) {
    eprintln!("Creating the SchemaDefinition  beep boop bapp");

    let schema_definition: Result<SchemaDefinition, _> =
        SchemaDefinition::try_from(&definition_document);

    eprintln!("schema_definition => {:?}", schema_definition);

    if let Ok(schema_def) = schema_definition {
        analyze_schema_definition(schema_def, query);
    } else {
        println!("Failed to create schema definition.");
    }
}

pub struct ScaleLimits;

impl
    bluejay_validator::executable::operation::Visitor<
        '_,
        ExecutableDocument<'_>,
        SchemaDefinition<'_>,
        serde_json::Map<String, serde_json::Value>,
    > for ScaleLimits
{
    fn new(
        operation_definition: &'_ <ExecutableDocument as bluejay_core::executable::ExecutableDocument>::OperationDefinition,
        schema_definition: &'_ SchemaDefinition,
        variable_values: &'_ serde_json::Map<String, serde_json::Value>,
        cache: &'_ bluejay_validator::executable::Cache<'_, ExecutableDocument, SchemaDefinition>,
    ) -> Self {
        Self
    }
}

impl
    bluejay_validator::executable::operation::Analyzer<
        '_,
        ExecutableDocument<'_>,
        SchemaDefinition<'_>,
        serde_json::Map<String, serde_json::Value>,
    > for ScaleLimits
{
    type Output = f64;

    fn into_output(self) -> Self::Output {
        1.0
    }
}

type ScaleLimitsAnalyzer<'a> = bluejay_validator::executable::operation::Orchestrator<
    'a,
    ExecutableDocument<'a>,
    SchemaDefinition<'a>,
    serde_json::Map<String, serde_json::Value>,
    ScaleLimits,
>;

fn analyze_schema_definition(schema_definition: SchemaDefinition, query: &str) {
    // create exeucatble document
    let executable_document =
        ExecutableDocument::parse(query).unwrap_or_else(|_| panic!("Document had parse errors"));
    let cache = bluejay_validator::executable::Cache::new(&executable_document, &schema_definition);

    let scale_factor = ScaleLimitsAnalyzer::analyze(
        &executable_document,
        &schema_definition,
        None,
        &Default::default(),
        &cache,
    )
    .unwrap();

    eprintln!(
        "Success creating ed, pass thing into analyzer? {:?}",
        scale_factor
    );
}

fn main() -> Result<()> {
    let opts: Opts = Opts::parse();

    let schema_string_result = opts.read_schema_to_string();

    let schema_string = match schema_string_result {
        Ok(schema_string) => schema_string,
        Err(error) => panic!("Problem creating schema from string"),
    };

    // validate
    let query_string = match opts.read_query_to_string() {
        Ok(query_string) => query_string,
        Err(error) => panic!("Problem creating schema from string"),
    };

    let document_definition: std::result::Result<DefinitionDocument, Vec<Error>> =
        create_definition_document(&schema_string);

    match document_definition {
        Ok(document) => {
            // If the document is successfully created, then continue with other stuff
            create_schema_definition(document, &query_string);
            println!("Document definition created successfully.");
            // Now we need to create the SchemaDefintiion, and thewn we can analyze it
        }
        Err(errors) => {
            // If there are errors, handle them
            for error in errors {
                eprintln!("Error parsing document: {:?}", error);
            }
            return Err(anyhow!("Failed to parse document."));
        }
    }

    let mut input: Box<dyn Read + Sync + Send + 'static> = if let Some(ref input) = opts.input {
        Box::new(BufReader::new(File::open(input).map_err(|e| {
            anyhow!("Couldn't load input {:?}: {}", input, e)
        })?))
    } else if !std::io::stdin().is_terminal() {
        Box::new(BufReader::new(stdin()))
    } else {
        return Err(anyhow!(
            "You must provide input via the --input flag or piped via stdin."
        ));
    };

    let mut buffer = Vec::new();
    input.read_to_end(&mut buffer)?;

    let buffer = match opts.codec {
        Codec::Json => {
            let _ = serde_json::from_slice::<serde_json::Value>(&buffer)
                .map_err(|e| anyhow!("Invalid input JSON: {}", e))?;
            buffer
        }
        Codec::Raw => buffer,
        Codec::JsonToMessagepack => {
            let json: serde_json::Value = serde_json::from_slice(&buffer)
                .map_err(|e| anyhow!("Invalid input JSON: {}", e))?;
            rmp_serde::to_vec(&json)
                .map_err(|e| anyhow!("Couldn't convert JSON to MessagePack: {}", e))?
        }
    };

    let profile_opts = opts.profile_opts();
    let function_run_result = run(FunctionRunParams {
        function_path: opts.function,
        input: buffer,
        export: opts.export.as_ref(),
        profile_opts: profile_opts.as_ref(),
    })?;

    if opts.json {
        println!("{}", function_run_result.to_json());
    } else {
        println!("{function_run_result}");
    }

    if let Some(profile) = function_run_result.profile.as_ref() {
        std::fs::write(profile_opts.unwrap().out, profile)?;
    }

    Ok(())
}
