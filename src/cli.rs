#![allow(unused_imports)]
#![allow(unused_variables)]

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Clone, Debug, Parser)]
pub struct CmdLineArgs {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Clone, Debug, Subcommand)]
pub enum Commands {
    #[command(name = "clsquery", arg_required_else_help = true)]
    ClsQuery {
        index_name: String,
        class_name: String,
    },

    #[command(name = "pkgenum", arg_required_else_help = true)]
    PkgEnum {
        index_name: String,
        package_name: String,
    },

    #[command(name = "dropindex", arg_required_else_help = true)]
    DropIndex {
        index_name: String,
    },

    #[command(arg_required_else_help = true)]
    Reindex {
        #[command(subcommand)]
        reindex_command: ReindexCommands,
    },

    Indexes,

    Enumerate {
        index_name: String,
    },

    Serve {
        socket_path: Option<String>,
    },
}

#[derive(Clone, Debug, Subcommand)]
pub enum ReindexCommands {
    #[command(arg_required_else_help = true)]
    Classpath {
        index_name: String,
        classpath_expr: String,
    },

    #[command(arg_required_else_help = true)]
    JarDir { index_name: String, jar_dir: String },

    #[command(arg_required_else_help = true)]
    JImage {
        index_name: String,
        image_file: String,
    },

    #[command(arg_required_else_help = true)]
    Project { index_name: String, src_dir: String },
}
