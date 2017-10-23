//
// @file main.rs
// @author Jacob Eberhardt <jacob.eberhardt@tu-berlin.de>
// @author Dennis Kuhnert <dennis.kuhnert@campus.tu-berlin.de>
// @date 2017

#![feature(box_patterns, box_syntax)]

extern crate clap;
#[macro_use]
extern crate lazy_static;
extern crate num; // cli
extern crate serde; // serialization deserialization
#[macro_use]
extern crate serde_derive;
extern crate bincode;

mod absy;
mod parser;
mod flatten;
mod r1cs;
mod field;
#[cfg(not(feature = "nolibsnark"))]
mod libsnark;

use std::fs::File;
use std::path::Path;
use std::io::{BufWriter, Write};
use field::{Field, FieldPrime};
use absy::Prog;
use parser::parse_program;
use flatten::Flattener;
use r1cs::r1cs_program;
use clap::{App, AppSettings, Arg, SubCommand};
#[cfg(not(feature = "nolibsnark"))]
use libsnark::run_libsnark;
use bincode::{serialize_into, deserialize_from , Infinite};

fn main() {
    const FLATTENED_CODE_DEFAULT_PATH: &str = "out";
    const VERIFICATION_KEY_DEFAULT_PATH: &str = "verification.key";
    const PROVING_KEY_DEFAULT_PATH: &str = "proving.key";
    const VERIFICATION_CONTRACT_DEFAULT_PATH: &str = "verifier.sol";
    const WITNESS_DEFAULT_PATH: &str = "witness";

    // cli specification using clap library
    let matches = App::new("ZoKrates")
    .setting(AppSettings::SubcommandRequiredElseHelp)
    .version("0.1")
    .author("Jacob Eberhardt, Dennis Kuhnert")
    .about("Supports generation of zkSNARKs from high level language code including Smart Contracts for proof verification on the Ethereum Blockchain.\n'I know that I show nothing!'")
    .subcommand(SubCommand::with_name("compile")
                                    .about("Compiles into flattened conditions. Produces two files: human-readable '.code' file and binary file")
                                    .arg(Arg::with_name("input")
                                        .short("i")
                                        .long("input")
                                        .help("path of source code file to compile.")
                                        .value_name("FILE")
                                        .takes_value(true)
                                        .required(true)
                                    ).arg(Arg::with_name("output")
                                        .short("o")
                                        .long("output")
                                        .help("output file path.")
                                        .value_name("FILE")
                                        .takes_value(true)
                                        .required(false)
                                        .default_value(FLATTENED_CODE_DEFAULT_PATH)
                                    )
                                 )
    .subcommand(SubCommand::with_name("setup")
        .about("Performs a trusted setup for a given constraint system.")
        .arg(Arg::with_name("input")
            .short("i")
            .long("input")
            .help("path of comiled code.")
            .value_name("FILE")
            .takes_value(true)
            .required(false)
            .default_value(FLATTENED_CODE_DEFAULT_PATH))
    )
    .subcommand(SubCommand::with_name("export-verifier")
        .about("Exports a verifier as Solidity smart contract.")
        .arg(Arg::with_name("input")
            .short("i")
            .long("input")
            .help("path of verifier.")
            .value_name("FILE")
            .takes_value(true)
            .required(false)
            .default_value(VERIFICATION_KEY_DEFAULT_PATH)
        )
        .arg(Arg::with_name("output")
            .short("o")
            .long("output")
            .help("output file path.")
            .value_name("FILE")
            .takes_value(true)
            .required(false)
            .default_value(VERIFICATION_CONTRACT_DEFAULT_PATH)
        )
    )
    .subcommand(SubCommand::with_name("compute-witness")
        .about("Calculates a witness for a given constraint system, i.e., a variable assignment which satisfies all constraints. Private inputs are specified interactively.")
        .arg(Arg::with_name("input")
            .short("i")
            .long("input")
            .help("path of comiled code.")
            .value_name("FILE")
            .takes_value(true)
            .required(false)
            .default_value(FLATTENED_CODE_DEFAULT_PATH)
        ).arg(Arg::with_name("output")
            .short("o")
            .long("output")
            .help("output file path.")
            .value_name("FILE")
            .takes_value(true)
            .required(false)
            .default_value(WITNESS_DEFAULT_PATH)
        ).arg(Arg::with_name("arguments")
            .short("a")
            .long("arguments")
            .help("Arguments for the program's main method. Space separated list.")
            .takes_value(true)
            .multiple(true) // allows multiple values
            .required(false)
        )
    )
    .subcommand(SubCommand::with_name("generate-proof")
        .about("Calculates a proof for a given constraint system and witness.")
        .arg(Arg::with_name("witness")
            .short("w")
            .long("witness")
            .help("path of witness file.")
            .value_name("FILE")
            .takes_value(true)
            .required(false)
            .default_value(WITNESS_DEFAULT_PATH)
        )
        .arg(Arg::with_name("provingkey")
            .short("pk")
            .long("provingkey")
            .help("path of proving key file.")
            .value_name("FILE")
            .takes_value(true)
            .required(false)
            .default_value(PROVING_KEY_DEFAULT_PATH)
        )
    )
    .subcommand(SubCommand::with_name("shortcut")
        .about("Executes witness generation, setup and proof-generation in succession")
        .arg(Arg::with_name("input")
            .short("i")
            .long("input")
            .help("path of comiled code.")
            .value_name("FILE")
            .takes_value(true)
            .required(false)
            .default_value(FLATTENED_CODE_DEFAULT_PATH)
        ).arg(Arg::with_name("arguments")
            .short("a")
            .long("arguments")
            .help("Arguments for the program's main method. Space separated list.")
            .takes_value(true)
            .multiple(true) // allows multiple values
            .required(false)
        )
    )
    .subcommand(SubCommand::with_name("deploy-verifier")
        .about("Deploys a given verification contract to the Ethereum network the current web3 provider is connected to.")
        .arg(Arg::with_name("input")
            .short("i")
            .long("input")
            .help("Solidity contract code.")
            .value_name("FILE")
            .takes_value(true)
            .required(true)
        ).arg(Arg::with_name("account")
            .short("acc")
            .long("account")
            .help("Address of the account triggering the Ethereum Transaction.")
            .takes_value(true)
        )
    )
    .get_matches();

    match matches.subcommand() {
        ("compile", Some(sub_matches)) => {
            println!("Compiling {}", sub_matches.value_of("input").unwrap());

            let path = Path::new(sub_matches.value_of("input").unwrap());
            let file = match File::open(&path) {
                Ok(file) => file,
                Err(why) => panic!("couldn't open {}: {}", path.display(), why),
            };

            let program_ast: Prog<FieldPrime> = match parse_program(file) {
                Ok(x) => x,
                Err(why) => {
                    println!("{:?}", why);
                    std::process::exit(1);
                }
            };

            // flatten input program
            let program_flattened =
                Flattener::new(FieldPrime::get_required_bits()).flatten_program(program_ast);

            // serialize flattened program and write to binary file
            let bin_output_path = Path::new(sub_matches.value_of("output").unwrap());
            let mut bin_output_file = match File::create(&bin_output_path) {
                Ok(file) => file,
                Err(why) => panic!("couldn't create {}: {}", bin_output_path.display(), why),
            };

            serialize_into(&mut bin_output_file, &program_flattened, Infinite).expect("Unable to write data to file.");

            // write human-readable output file
            let hr_output_path = bin_output_path.to_path_buf().with_extension("code");

            let hr_output_file = match File::create(&hr_output_path) {
                Ok(file) => file,
                Err(why) => panic!("couldn't create {}: {}", hr_output_path.display(), why),
            };

            let mut hrofb = BufWriter::new(hr_output_file);
            write!(&mut hrofb, "{}\n", program_flattened).expect("Unable to write data to file.");
            hrofb.flush().expect("Unable to flush buffer.");

            // debugging output
            println!("Compiled program:\n{}", program_flattened);
            println!(
                "Compiled code written to {}",
                sub_matches.value_of("output").unwrap()
            );
        }
        ("compute-witness", Some(sub_matches)) => {
            println!("Computing witness for:");

            // read compiled program
            let path = Path::new(sub_matches.value_of("input").unwrap());
            let mut file = match File::open(&path) {
                Ok(file) => file,
                Err(why) => panic!("couldn't open {}: {}", path.display(), why),
            };

            let program_ast: Prog<FieldPrime> = match deserialize_from(&mut file, Infinite) {
                Ok(x) => x,
                Err(why) => {
                    println!("{:?}", why);
                    std::process::exit(1);
                }
            };

            // make sure the input program is actually flattened.
            // TODO: is_flattened should be provided as method of Prog in absy.
            let main_flattened = program_ast
                .functions
                .iter()
                .find(|x| x.id == "main")
                .unwrap();
            for stat in main_flattened.statements.clone() {
                assert!(
                    stat.is_flattened(),
                    format!("Input conditions not flattened: {}", &stat)
                );
            }

            // print deserialized flattened program
            println!("{}", main_flattened);

            // validate #arguments
            let mut args: Vec<FieldPrime> = Vec::new();
            match sub_matches.values_of("arguments"){
                Some(p) => {
                    let arg_strings: Vec<&str> = p.collect();
                    args = arg_strings.into_iter().map(|x| FieldPrime::from(x)).collect();
                },
                None => {
                }
            }

            if main_flattened.arguments.len() != args.len() {
                println!("Wrong number of arguments. Given: {}, Required: {}.", args.len(), main_flattened.arguments.len());
                std::process::exit(1);
            }

            let witness_map = main_flattened.get_witness(args);
            println!("Witness: {:?}", witness_map);
            match witness_map.get("~out") {
                Some(out) => println!("Returned (~out): {}", out),
                None => println!("~out not found, no value returned")
            }

            // write witness to file
            let output_path = Path::new(sub_matches.value_of("output").unwrap());
            let mut output_file = match File::create(&output_path) {
                Ok(file) => file,
                Err(why) => panic!("couldn't create {}: {}", output_path.display(), why),
            };
            let mut bw = BufWriter::new(output_file);
            write!(&mut bw, "{:?}\n", witness_map).expect("Unable to write data to file.");
            bw.flush().expect("Unable to flush buffer.");

        }
        ("shortcut", Some(sub_matches)) => {
            println!("Performing setup...");
            // read compiled program
            let path = Path::new(sub_matches.value_of("input").unwrap());
            let mut file = match File::open(&path) {
                Ok(file) => file,
                Err(why) => panic!("couldn't open {}: {}", path.display(), why),
            };

            let program_ast: Prog<FieldPrime> = match deserialize_from(&mut file, Infinite) {
                Ok(x) => x,
                Err(why) => {
                    println!("{:?}", why);
                    std::process::exit(1);
                }
            };

            // make sure the input program is actually flattened.
            // TODO: is_flattened should be provided as method of Prog in absy.
            let main_flattened = program_ast
                .functions
                .iter()
                .find(|x| x.id == "main")
                .unwrap();
            for stat in main_flattened.statements.clone() {
                assert!(
                    stat.is_flattened(),
                    format!("Input conditions not flattened: {}", &stat)
                );
            }

            // print deserialized flattened program
            println!("{}", main_flattened);

            // validate #arguments
            let mut args: Vec<FieldPrime> = Vec::new();
            match sub_matches.values_of("arguments"){
                Some(p) => {
                    let arg_strings: Vec<&str> = p.collect();
                    args = arg_strings.into_iter().map(|x| FieldPrime::from(x)).collect();
                },
                None => {
                }
            }

            if main_flattened.arguments.len() != args.len() {
                println!("Wrong number of arguments. Given: {}, Required: {}.", args.len(), main_flattened.arguments.len());
                std::process::exit(1);
            }

            let witness_map = main_flattened.get_witness(args);
            println!("Witness: {:?}", witness_map);
            match witness_map.get("~out") {
                Some(out) => println!("Returned (~out): {}", out),
                None => println!("~out not found, no value returned")
            }

            let (variables, a, b, c) = r1cs_program(&program_ast);

            let witness: Vec<_> = variables.iter().map(|x| witness_map[x].clone()).collect();

            // run libsnark
            #[cfg(not(feature="nolibsnark"))]{
                // number of inputs in the zkSNARK sense, i.e., input variables + output variables
                let num_inputs = main_flattened.arguments.len() + 1; //currently exactly one output variable
                println!("run_libsnark = {:?}", run_libsnark(variables, a, b, c, witness, num_inputs));
            }
        }
        ("export-verifier", Some(_)) => {
            println!("Exporting verifier...");
            //TODO
        }
        ("generate-proof", Some(_)) => {
            println!("Generating proof...");
            //TODO
        }
        ("deploy-verifier", Some(_)) => {
            println!("Deploying verifier...");
            //TODO Steffen
            // use https://github.com/tomusdrw/rust-web3 for blockchain interaction
            // and https://doc.rust-lang.org/std/process/struct.Command.html for solc
        }
        _ => unimplemented!(), // Either no subcommand or one not tested for...
    }

}

#[cfg(test)]
mod tests {
    extern crate glob;
    use super::*;
    use num::Zero;
    use self::glob::glob;

    #[test]
    fn examples() {
        for p in glob("./examples/*.code").expect("Failed to read glob pattern") {
            let path = match p {
                Ok(x) => x,
                Err(why) => panic!("Error: {:?}", why),
            };
            println!("Testing {:?}", path);
            let file = match File::open(&path) {
                Ok(file) => file,
                Err(why) => panic!("couldn't open {:?}: {}", path, why),
            };

            let program_ast = match parse_program::<FieldPrime>(file) {
                Ok(x) => x,
                Err(why) => panic!("Error: {:?}", why),
            };
            let program_flattened =
                Flattener::new(FieldPrime::get_required_bits()).flatten_program(program_ast);
            let (..) = r1cs_program(&program_flattened);
        }
    }

    #[test]
    fn examples_with_input() {
        for p in glob("./examples/test*.code").expect("Failed to read glob pattern") {
            let path = match p {
                Ok(x) => x,
                Err(why) => panic!("Error: {:?}", why),
            };
            println!("Testing {:?}", path);
            let file = match File::open(&path) {
                Ok(file) => file,
                Err(why) => panic!("couldn't open {:?}: {}", path, why),
            };

            let program_ast = match parse_program::<FieldPrime>(file) {
                Ok(x) => x,
                Err(why) => panic!("Error: {:?}", why),
            };
            let program_flattened =
                Flattener::new(FieldPrime::get_required_bits()).flatten_program(program_ast);
            let (..) = r1cs_program(&program_flattened);
            let _ = program_flattened.get_witness(vec![FieldPrime::zero()]);
        }
    }
}
