extern crate regex;
extern crate serde_json;

use std::io::{Read, Write};
use regex::Regex;

// TODO: support more types
static PERMITTED_C_TYPES: [&'static str; 3] = ["char", "char*", "int32"];

#[derive(Deserialize)]
struct PItem {
    c_var: String,
    c_type: String,
    help: Option<String>,
}

#[derive(Deserialize)]
struct NPItem {
    c_var: String,
    c_type: String,
    name: String,
    short: Option<String>,
    aliases: Option<Vec<String>>,
    help: Option<String>,
    required: Option<bool>,
    default: Option<String>,
}

impl NPItem {
    /// declarations for the main function.
    fn decl_main(&self) -> String {
        format!("\t{} {};\n", self.c_type, self.c_var)
    }
    /// declarations for the parse_args (not main) function.
    fn decl_parse(&self) -> String {
        format!("\tbool {}__isset = false;\n", self.c_var)
    }
    /// generate appropriate C code for the particular argument, to be contained within the primary
    /// argument loop. Assume that c_var is an initially-null pointer to a c_type, and
    /// c_var+"__isset" is a boolean. This function should make c_var non-null if applicable, and
    /// if so it sohuld set c_var+"__isset" to true.
    fn gen(&self) -> String {
        let mut code = String::new();
        // TODO: There's a special case for binary args like --verbose where there's no subsequent
        // arg. Also, we should add support for --foo=bar on top of just --foo bar
        code.push_str(&format!("\t\tif (!strcmp(argv[i], \"--{}\") && i+1<argc) {{\n",
                               self.name));
        match &*self.c_type { // TODO: int arrays, string array
            "int32" => code.push_str(&format!("\t\t\t*{} = atoi(argv[++i]);\n", self.c_var)),
            "char*" => code.push_str(&format!("\t\t\t*{} = argv[++i];\n", self.c_var)),
            "char"  => code.push_str(&format!("\t\t\t*{} = argv[++i][0];\n", self.c_var)),
            _ => ()/* impossible (due to sanity check) */,
        }
        code.push_str(&format!("\t\t\t{}__isset = true;\n", self.c_var));
        code.push_str("\t\t\targ_count += 2;\n");
        code.push_str("\t\t}\n");
        code
    }
    /// generate appropriate C code for after the the primary argument loop. This should check the
    /// c_var+"__isset" value, and if it is false it should either cause the C program to fail with
    /// the help menu or it should assign a default value for c_var. After this is called, if the
    /// program is still running, then c_var MUST be set appropriately.
    fn post_loop(&self) -> String {
        let mut code = String::new();
        code.push_str(&format!("\tif (!{}__isset) {{\n", self.c_var));
        if self.required.unwrap_or(false) {
            code.push_str("\t\tusage(argv[0]);\n");
            code.push_str("\t\texit(1);\n");
        } else if let Some(ref default) = self.default {
            match &*self.c_type {
                "int32" => code.push_str(&format!("\t\t*{} = {};\n", self.c_var, default)),
                // TODO: handle quoting correctly for char* AND char
                "char*" => code.push_str(&format!("\t\t*{} = \"{}\";\n", self.c_var, default)),
                "char"  => code.push_str(&format!("\t\t*{} = '{}';\n", self.c_var, default)),
                _ => ()/* impossible */,
            }
        }
        code.push_str("\t}\n");
        code
    }
}


impl PItem {
    /// declarations for the main function.
    fn decl(&self) -> String {
        format!("\t{} {};\n", self.c_type, self.c_var)
    }

    fn gen(&self) -> String {
        String::new()
    }

    fn post_loop(&self) -> String {
        String::new()
    }
}

#[derive(Deserialize)]
pub struct Spec {
    positional: Vec<PItem>,
    non_positional: Vec<NPItem>,
}

impl Spec {
    /// deserializes json from a reader into a Spec.
    pub fn from_reader<R>(rdr: R) -> Spec
        where R: Read
    {
        let s: Spec = serde_json::from_reader(rdr).expect("parse json argument spec");
        s.sanity_check(); // panic if nonsense input
        s
    }
    /// check all items in the spec to make sure they are valid.
    fn sanity_check(&self) {
        let identifier_re = Regex::new(r"^[_a-zA-Z][_a-zA-Z0-9]*$").unwrap();
        for pi in &self.positional {
            assert!(identifier_re.is_match(&pi.c_var),
                    format!("invalid c variable \"{}\"", pi.c_var));
            let valid_type = (&PERMITTED_C_TYPES)
                .into_iter()
                .any(|&tp| tp == pi.c_type);
            assert!(valid_type, format!("invalid c type: \"{}\"", pi.c_type));
        }
        for pi in &self.non_positional {
            assert!(identifier_re.is_match(&pi.c_var),
                    format!("invalid c variable \"{}\"", pi.c_var));
            let valid_type = (&PERMITTED_C_TYPES)
                .into_iter()
                .any(|&tp| tp == pi.c_type);
            assert!(valid_type, format!("invalid c type: \"{}\"", pi.c_type));
            assert!(pi.name.find(' ').is_none(),
                    "invalid argument name: \"{}\"",
                    pi.name);
            if let Some(ref short_name) = pi.short {
                assert!(short_name.len() == 1,
                        "invalid short name: \"{}\"",
                        short_name);
            }
            if let Some(ref aliases) = pi.aliases {
                for alias in aliases {
                    assert!(alias.find(' ').is_none(),
                            "invalid argument alias name: \"{}\"",
                            alias);
                }
            }
        }
    }
    /// creates the necessary headers in C.
    fn c_headers(&self) -> String {
        String::from("#include<stdlib.h>\n#include<stdio.h>\n#include<string.h>")
    }
    /// creates the usage function in C.
    fn c_usage(&self) -> String {
        // TODO: positional usage. escape double quotes in help message.
        let positional_usage = "[TODO ...]";
        let mut help = String::from("  -h  --help\n        print this usage and exit\n");
        help.push_str(&self.non_positional
                           .iter()
                           .map(|ref npi| {
            let mut long = String::from("  --");
            long.push_str(&npi.name);
            if let Some(ref aliases) = npi.aliases {
                for alias in aliases {
                    long.push_str("  --");
                    long.push_str(alias);
                }
            }
            let help = match npi.help {
                Some(ref h) => {
                    let mut hm = String::from("\n        ");
                    hm.push_str(h);
                    hm
                }
                _ => String::new(),
            };
            if let Some(ref short) = npi.short {
                format!("  -{}{}{}\n", short, long, help)
            } else {
                format!("     {}{}\n", long, help)
            }
        })
                           .collect::<String>());
        format!(r#"static void usage(const char *progname) {{
	printf("usage: %s [options] {}\n%s", progname, "\
{}");
}}
"#,
                positional_usage,
                help)
    }
    /// creates the parse_args function in C.
    fn c_parse_args(&self) -> String {
        let mut body = String::new();
        body.push_str("void parse_args(int argc, char **argv /* TODO */) {\n");

        // TODO: if using glibc, use getopt.h to automate most of this

        // create c_var+"_isset" booleans
        for npi in &self.non_positional {
            body.push_str(&npi.decl_parse());
        }

        // push arg_count variable, which will be used for positional arguments
        body.push_str("\tint arg_count = 0;\n");

        // primary loop npitem
        body.push_str("\tfor (int i = 1; i < argc; i++) {\n");

        // TODO: Add condition for checking whether we have gotten past all positional arguments
        for npi in &self.non_positional {
            body.push_str(&npi.gen());
        }
        body.push_str("\t}\n");

        // primary loop for pitem
        body.push_str("\tfor (int i = arg_count; i < argc; i++) {\n");
        for pi in &self.positional {
            body.push_str(&pi.gen());
        }
        body.push_str("\t}\n");

        // post_loop
        for pi in &self.positional {
            body.push_str(&pi.post_loop()); // TODO: Pass relative position index into pi.post_loop
        }
        for npi in &self.non_positional {
            body.push_str(&npi.post_loop());
        }

        body.push_str("}\n");
        body
    }
    /// creates the main function in C.
    fn c_main(&self) -> String {
        let mut main = String::new();
        main.push_str("int main(int argc, char **argv) {\n");

        for pi in &self.positional {
            main.push_str(&pi.decl())
        }
        for npi in &self.non_positional {
            main.push_str(&npi.decl_main())
        }

        main.push_str("\n\tparse_args(argc, argv");
        for pi in &self.positional {
            main.push_str(&format!(", &{}", pi.c_var))
        }
        for npi in &self.non_positional {
            main.push_str(&format!(", &{}", npi.c_var))
        }
        main.push_str(");\n\n");

        main.push_str("\t/* TODO: call your code here */\n");
        main.push_str("}\n");
        main
    }
    /// generates argen.c which features the function argen.
    pub fn gen(&self) -> String {
        let h = self.c_headers();
        let usage = self.c_usage();
        let body = self.c_parse_args();
        let main = self.c_main();
        format!("{}\n\n{}\n{}\n{}", h, usage, body, main)
        // TODO: Add Main function
    }
    /// writes generate C code to a writer.
    pub fn writeout<W>(&self, wrt: &mut W)
        where W: Write
    {
        wrt.write_all(self.gen().as_bytes())
            .expect("write generated code to file")
    }
}