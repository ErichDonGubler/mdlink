use std::{fs, sync::Arc};

use camino::Utf8PathBuf;
use rune::termcolor::*;

use crate::AlreadyReportedToCommandLine;

pub(crate) struct ScriptingEngine {
    vm: rune::Vm,
    sources: rune::Sources,
    stderr: StandardStream,
}

impl ScriptingEngine {
    pub(crate) fn new(script: Utf8PathBuf) -> Result<Self, AlreadyReportedToCommandLine> {
        let source = fs::read_to_string(&script).unwrap();

        let mut context = rune::Context::with_default_modules().unwrap();

        let mut module = rune::Module::with_crate("mdlink").unwrap();
        module.ty::<ScriptArgs>().unwrap();
        context.install(module).unwrap();

        let mut module = rune::Module::with_crate("url").unwrap();
        module.ty::<Url>().unwrap();
        module.function_meta(Url::display_fmt).unwrap();
        module.function_meta(Url::scheme__meta).unwrap();
        module.function_meta(Url::host__meta).unwrap();
        module.function_meta(Url::path_segments__meta).unwrap();
        module.function_meta(Url::query_pairs__meta).unwrap();
        module.function_meta(Url::fragment__meta).unwrap();
        context.install(module).unwrap();

        let runtime = Arc::new(context.runtime().unwrap());

        let mut sources = rune::Sources::new();
        let source = match rune::Source::with_path(&script, source, &script) {
            Ok(ok) => ok,
            Err(e) => {
                eprintln!("failed to load script: {e}");
                return Err(AlreadyReportedToCommandLine);
            }
        };
        sources.insert(source).unwrap();

        let mut diagnostics = rune::Diagnostics::new();

        let result = rune::prepare(&mut sources)
            .with_context(&context)
            .with_diagnostics(&mut diagnostics)
            .build();

        let mut stderr = StandardStream::stderr(ColorChoice::Auto);
        if !diagnostics.is_empty() {
            diagnostics.emit(&mut stderr, &sources).unwrap();
        };

        let unit = if diagnostics.has_error() {
            return Err(AlreadyReportedToCommandLine);
        } else {
            result.unwrap()
        };

        let vm = rune::Vm::new(runtime, Arc::new(unit));

        Ok(Self {
            vm,
            sources,
            stderr,
        })
    }

    pub fn run(&mut self, input: url::Url) -> Result<String, AlreadyReportedToCommandLine> {
        let Self {
            vm,
            ref sources,
            stderr,
        } = self;

        let res = vm.call(
            ["main"],
            (ScriptArgs {
                url: Url { inner: input },
            },),
        );
        let output = match res {
            Ok(ok) => ok,
            Err(e) => {
                e.emit(stderr, sources).unwrap();
                return Err(AlreadyReportedToCommandLine);
            }
        };
        rune::from_value::<String>(output).map_err(|e| {
            e.emit(stderr, sources).unwrap();
            AlreadyReportedToCommandLine
        })
    }
}

#[derive(Debug, rune::Any)]
#[rune(item = ::mdlink)]
struct ScriptArgs {
    #[rune(get)]
    url: Url,
}

#[derive(Clone, Debug, rune::Any)]
#[rune(item = ::url)]
struct Url {
    inner: url::Url,
}

impl Url {
    #[rune::function(protocol = STRING_DISPLAY)]
    fn display_fmt(&self, f: &mut rune::runtime::Formatter) -> rune::runtime::VmResult<()> {
        use rune::alloc::fmt::TryWrite;
        rune::vm_write!(f, "{}", self.inner);
        rune::runtime::VmResult::Ok(())
    }

    #[rune::function(instance, keep)]
    fn scheme(&self) -> String {
        self.inner.scheme().to_owned()
    }

    #[rune::function(instance, keep)]
    fn host(&self) -> Option<String> {
        self.inner.host_str().map(|s| s.to_owned())
    }

    #[rune::function(instance, keep)]
    fn path_segments(&self) -> Option<rune::runtime::Vec> {
        // TODO: custom structure with `Iterator` trait implemented instead, for perf.
        let path_segments = self
            .inner
            .path_segments()?
            .map(|s| s.to_owned())
            .map(rune::to_value)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        Some(path_segments.try_into().unwrap())
    }

    #[rune::function(instance, keep)]
    fn query_pairs(&self) -> rune::runtime::Vec {
        // TODO: custom structure with `Iterator` trait implemented instead, for perf.
        let query_pairs = self
            .inner
            .query_pairs()
            .map(|(key, val)| (key.into_owned(), val.into_owned()))
            .map(rune::to_value)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        query_pairs.try_into().unwrap()
    }

    #[rune::function(instance, keep)]
    fn fragment(&self) -> Option<String> {
        self.inner.fragment().map(|s| s.to_owned())
    }
}
