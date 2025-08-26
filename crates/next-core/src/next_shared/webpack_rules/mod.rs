use std::{collections::BTreeSet, fmt, str::FromStr, sync::LazyLock};

use anyhow::Result;
use serde::{Deserialize, Serialize, de::Visitor};
use turbo_rcstr::rcstr;
use turbo_tasks::{NonLocalValue, OperationValue, ResolvedVc, TaskInput, Vc, trace::TraceRawVcs};
use turbo_tasks_fs::FileSystemPath;
use turbopack::module_options::{
    WebpackLoaderBuiltinConditionSet, WebpackLoaderBuiltinConditionSetMatch, WebpackLoadersOptions,
};
use turbopack_core::resolve::{ExternalTraced, ExternalType, options::ImportMapping};

use self::{babel::maybe_add_babel_loader, sass::add_sass_loader};
use crate::next_config::NextConfig;

pub(crate) mod babel;
pub(crate) mod sass;

/// Built-in conditions provided by the Next.js Turbopack integration for configuring webpack
/// loaders. These can be used in the `next.config.js` `turbopack.rules` section.
///
/// These are different from than the user-configurable "conditions" field.
//
// Note: If you add a field here, make sure to also add it in:
// - The typescript definition in `packages/next/src/server/config-shared.ts`
// - The zod schema in `packages/next/src/server/config-schema.ts`
//
// Note: Sets of conditions could be stored more efficiently as a bitset, but it's probably not used
// in enough places for it to matter.
#[derive(
    Copy,
    Clone,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    TaskInput,
    TraceRawVcs,
    NonLocalValue,
    OperationValue,
)]
pub enum WebpackLoaderBuiltinCondition {
    /// Treated as always-present.
    Default,
    /// Client-side code.
    Browser,
    /// Code in `node_modules` that should typically not be modified by webpack loaders.
    Foreign,

    // These are provided by NextMode:
    Development,
    Production,

    // These are provided by NextRuntime:
    /// Server code on the Node.js runtime.
    Node,
    /// Server code on the edge runtime.
    EdgeLight,
}

impl WebpackLoaderBuiltinCondition {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Browser => "browser",
            Self::Foreign => "foreign",
            Self::Development => "development",
            Self::Production => "production",
            Self::Node => "node",
            Self::EdgeLight => "edge-light",
        }
    }

    fn all_conditions() -> &'static [Self] {
        &[
            Self::Default,
            Self::Browser,
            Self::Foreign,
            Self::Development,
            Self::Production,
            Self::Node,
            Self::EdgeLight,
        ]
    }

    fn all_condition_strs() -> &'static [&'static str] {
        static VALUE: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
            WebpackLoaderBuiltinCondition::all_conditions()
                .iter()
                .map(|c| c.as_str())
                .collect::<Vec<_>>()
        });
        &VALUE
    }
}

impl FromStr for WebpackLoaderBuiltinCondition {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "default" => Ok(Self::Default),
            "browser" => Ok(Self::Browser),
            "foreign" => Ok(Self::Foreign),
            "development" => Ok(Self::Development),
            "production" => Ok(Self::Production),
            "node" => Ok(Self::Node),
            "edge-light" => Ok(Self::EdgeLight),
            _ => Err(()),
        }
    }
}

impl Serialize for WebpackLoaderBuiltinCondition {
    fn serialize<S>(&self, ser: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        ser.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for WebpackLoaderBuiltinCondition {
    fn deserialize<D>(de: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct ConditionVisitor;
        impl<'de> Visitor<'de> for ConditionVisitor {
            type Value = WebpackLoaderBuiltinCondition;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                let valid_conditions =
                    WebpackLoaderBuiltinCondition::all_condition_strs().join(", ");

                write!(f, "one of the following strings: {valid_conditions}")
            }

            fn visit_str<E>(self, v: &str) -> std::result::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                v.parse().map_err(|_| {
                    serde::de::Error::unknown_variant(
                        v,
                        WebpackLoaderBuiltinCondition::all_condition_strs(),
                    )
                })
            }
        }
        de.deserialize_str(ConditionVisitor)
    }
}

impl PartialEq<WebpackLoaderBuiltinCondition> for &str {
    fn eq(&self, other: &WebpackLoaderBuiltinCondition) -> bool {
        *self == other.as_str()
    }
}

#[turbo_tasks::value]
struct NextWebpackLoaderBuiltinConditionSet(BTreeSet<WebpackLoaderBuiltinCondition>);

#[turbo_tasks::value_impl]
impl NextWebpackLoaderBuiltinConditionSet {
    #[turbo_tasks::function]
    fn new(
        conditions: BTreeSet<WebpackLoaderBuiltinCondition>,
    ) -> Vc<Box<dyn WebpackLoaderBuiltinConditionSet>> {
        Vc::upcast::<Box<dyn WebpackLoaderBuiltinConditionSet>>(
            NextWebpackLoaderBuiltinConditionSet(conditions).cell(),
        )
    }
}

#[turbo_tasks::value_impl]
impl WebpackLoaderBuiltinConditionSet for NextWebpackLoaderBuiltinConditionSet {
    fn match_condition(&self, condition: &str) -> WebpackLoaderBuiltinConditionSetMatch {
        match WebpackLoaderBuiltinCondition::from_str(condition) {
            Ok(cond) => {
                if self.0.contains(&cond) {
                    WebpackLoaderBuiltinConditionSetMatch::Matched
                } else {
                    WebpackLoaderBuiltinConditionSetMatch::Unmatched
                }
            }
            Err(_) => WebpackLoaderBuiltinConditionSetMatch::Invalid,
        }
    }
}

pub async fn webpack_loader_options(
    project_path: FileSystemPath,
    next_config: Vc<NextConfig>,
    builtin_conditions: BTreeSet<WebpackLoaderBuiltinCondition>,
) -> Result<Option<ResolvedVc<WebpackLoadersOptions>>> {
    let mut rules = next_config.webpack_rules(project_path.clone());

    if !*next_config
        .experimental_turbopack_force_disable_sass()
        .await?
    {
        rules = add_sass_loader(next_config.sass_config(), rules);
    }

    if !*next_config
        .experimental_turbopack_force_disable_babel()
        .await?
    {
        rules = maybe_add_babel_loader(project_path.clone(), rules);
    }

    let rules = rules.to_resolved().await?;
    Ok(if !rules.await?.is_empty() {
        let conditions = next_config.webpack_conditions().to_resolved().await?;
        Some(
            WebpackLoadersOptions {
                rules,
                conditions,
                loader_runner_package: Some(loader_runner_package_mapping().to_resolved().await?),
                builtin_conditions: NextWebpackLoaderBuiltinConditionSet::new(builtin_conditions)
                    .to_resolved()
                    .await?,
            }
            .resolved_cell(),
        )
    } else {
        None
    })
}

#[turbo_tasks::function]
fn loader_runner_package_mapping() -> Result<Vc<ImportMapping>> {
    Ok(ImportMapping::Alternatives(vec![
        ImportMapping::External(
            Some(rcstr!("next/dist/compiled/loader-runner")),
            ExternalType::CommonJs,
            ExternalTraced::Untraced,
        )
        .resolved_cell(),
    ])
    .cell())
}
