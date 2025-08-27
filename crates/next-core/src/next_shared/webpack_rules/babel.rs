use anyhow::Result;
use turbo_rcstr::rcstr;
use turbo_tasks::{ResolvedVc, Vc};
use turbo_tasks_fs::{self, FileSystemEntryType, FileSystemPath};
use turbopack::module_options::{LoaderRuleItem, WebpackRules};
use turbopack_core::{
    issue::{Issue, IssueExt, IssueSeverity, IssueStage, OptionStyledString, StyledString},
    reference_type::{CommonJsReferenceSubType, ReferenceType},
    resolve::{node::node_cjs_resolve_options, parse::Request, pattern::Pattern, resolve},
};
use turbopack_node::transforms::webpack::WebpackLoaderItem;

// https://babeljs.io/docs/config-files
// TODO: Also support a `babel` key in a package.json file
const BABEL_CONFIG_FILES: &[&str] = &[
    ".babelrc",
    ".babelrc.json",
    ".babelrc.js",
    ".babelrc.mjs",
    ".babelrc.cjs",
    "babel.config.js",
    "babel.config.json",
    "babel.config.mjs",
    "babel.config.cjs",
];

/// If the user has a babel configuration file (see list above) alongside their `next.config.js`
/// configuration, automatically add `babel-loader` as a webpack loader for each eligible file type
/// if it doesn't already exist.
#[turbo_tasks::function]
pub async fn maybe_add_babel_loader(
    project_root: FileSystemPath,
    webpack_rules: Vc<WebpackRules>,
) -> Result<Vc<WebpackRules>> {
    let mut has_babel_config = false;
    for &filename in BABEL_CONFIG_FILES {
        let filetype = *project_root.join(filename)?.get_type().await?;
        if matches!(filetype, FileSystemEntryType::File) {
            has_babel_config = true;
            break;
        }
    }
    if !has_babel_config {
        return Ok(webpack_rules);
    }

    let mut rules = webpack_rules.owned().await?;
    let pattern = rcstr!("*.{js,jsx,ts,tsx,cjs,mjs,mts,cts}");

    if !*is_babel_loader_available(project_root.clone()).await? {
        BabelIssue {
            path: project_root.clone(),
            title: StyledString::Text(rcstr!(
                "Unable to resolve babel-loader, but a babel config is present"
            ))
            .resolved_cell(),
            description: StyledString::Text(rcstr!(
                "Make sure babel-loader is installed via your package manager."
            ))
            .resolved_cell(),
            severity: IssueSeverity::Fatal,
        }
        .resolved_cell()
        .emit();
    }

    rules.push((
        pattern,
        LoaderRuleItem {
            loaders: ResolvedVc::cell(vec![WebpackLoaderItem {
                loader: rcstr!("babel-loader"),
                options: Default::default(),
            }]),
            rename_as: Some(rcstr!("*")),
            condition: None,
        },
    ));

    Ok(Vc::cell(rules))
}

#[turbo_tasks::function]
pub async fn is_babel_loader_available(project_path: FileSystemPath) -> Result<Vc<bool>> {
    let result = resolve(
        project_path.clone(),
        ReferenceType::CommonJs(CommonJsReferenceSubType::Undefined),
        Request::parse(Pattern::Constant(rcstr!("babel-loader/package.json"))),
        node_cjs_resolve_options(project_path),
    );
    let assets = result.primary_sources().await?;
    Ok(Vc::cell(!assets.is_empty()))
}

#[turbo_tasks::value]
struct BabelIssue {
    path: FileSystemPath,
    title: ResolvedVc<StyledString>,
    description: ResolvedVc<StyledString>,
    severity: IssueSeverity,
}

#[turbo_tasks::value_impl]
impl Issue for BabelIssue {
    #[turbo_tasks::function]
    fn stage(&self) -> Vc<IssueStage> {
        IssueStage::Transform.into()
    }

    fn severity(&self) -> IssueSeverity {
        self.severity
    }

    #[turbo_tasks::function]
    fn file_path(&self) -> Vc<FileSystemPath> {
        self.path.clone().cell()
    }

    #[turbo_tasks::function]
    fn title(&self) -> Vc<StyledString> {
        *self.title
    }

    #[turbo_tasks::function]
    fn description(&self) -> Vc<OptionStyledString> {
        Vc::cell(Some(self.description))
    }
}
