//! Internationalization (i18n) for MemPalace.
//!
//! A minimal, pure-Rust translation layer for the MemPalace CLI. It supports
//! locale detection from environment variables, a dotted-key translation lookup,
//! simple `{var}` interpolation, localized greeting/help strings, and a basic
//! pluralization helper.
//!
//! # Examples
//!
//! ```
//! use mempalace_rs::i18n::{Locale, I18n};
//!
//! let i18n = I18n::new(Locale::En);
//! assert_eq!(i18n.t("terms.wing"), "wing");
//! assert_eq!(i18n.greeting(None), "Welcome to MemPalace!");
//! ```

use lazy_static::lazy_static;
use std::collections::HashMap;

/// Supported locales.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Locale {
    /// English (default)
    #[default]
    En,
    /// Spanish
    Es,
    /// French
    Fr,
    /// German
    De,
    /// Brazilian Portuguese
    PtBr,
    /// Simplified Chinese
    ZhCn,
    /// Japanese
    Ja,
    /// Russian
    Ru,
    /// Belarusian
    Be,
}

impl Locale {
    /// Return the canonical two-letter (or BCP 47) code for this locale.
    pub fn as_str(&self) -> &'static str {
        match self {
            Locale::En => "en",
            Locale::Es => "es",
            Locale::Fr => "fr",
            Locale::De => "de",
            Locale::PtBr => "pt-BR",
            Locale::ZhCn => "zh-CN",
            Locale::Ja => "ja",
            Locale::Ru => "ru",
            Locale::Be => "be",
        }
    }

    /// Resolve a language tag to a supported locale.
    ///
    /// Accepts bare codes (`en`, `es`), region variants (`en-US`, `pt-BR`,
    /// `zh-CN`, `zh-TW`), and case-insensitive input.
    pub fn from_code(code: &str) -> Option<Self> {
        let code = code.trim();
        if code.is_empty() {
            return None;
        }
        // Extract the primary language subtag, ignoring the region/script.
        let normalized = code
            .split_once(&['-', '_'][..])
            .map(|(s, _)| s)
            .unwrap_or(code);
        let normalized = normalized.to_ascii_lowercase();
        match normalized.as_str() {
            "en" => Some(Locale::En),
            "es" => Some(Locale::Es),
            "fr" => Some(Locale::Fr),
            "de" => Some(Locale::De),
            "pt" => Some(Locale::PtBr),
            "zh" => Some(Locale::ZhCn),
            "ja" => Some(Locale::Ja),
            "ru" => Some(Locale::Ru),
            "be" => Some(Locale::Be),
            _ => None,
        }
    }

    /// Return the default fallback locale.
    pub fn default_locale() -> Self {
        Locale::En
    }
}

/// Internal container for a locale's translation strings.
struct TranslationSet {
    map: HashMap<&'static str, &'static str>,
}

impl TranslationSet {
    fn get(&self, key: &str) -> Option<&'static str> {
        self.map.get(key).copied()
    }
}

lazy_static! {
    static ref TRANSLATIONS: HashMap<Locale, TranslationSet> = {
        let mut map = HashMap::new();
        map.insert(Locale::En, build_en());
        map.insert(Locale::Es, build_es());
        map.insert(Locale::Fr, build_fr());
        map.insert(Locale::De, build_de());
        map.insert(Locale::PtBr, build_pt_br());
        map.insert(Locale::ZhCn, build_zh_cn());
        map.insert(Locale::Ja, build_ja());
        map.insert(Locale::Ru, build_ru());
        map.insert(Locale::Be, build_be());
        map
    };
}

fn build_en() -> TranslationSet {
    let mut map = HashMap::new();
    map.insert("greeting", "Welcome to MemPalace!");
    map.insert("greeting_named", "Welcome to MemPalace, {name}!");
    map.insert(
        "help",
        "MemPalace — offline-first AI memory\n\
        \n\
        Usage: mempalace <COMMAND>\n\
        \n\
        Common commands:\n\
          init <dir>        Detect rooms from your folder structure\n\
          mine <dir>        Mine files into the palace\n\
          search <query>      Find anything with exact or semantic search\n\
          status            Show what has been filed\n\
          repair            Re-index entries into vector storage\n\
          instructions      Print system prompts for agents\n\
        \n\
        Set MEMPALACE_LANG to override the locale (e.g. en, es, fr).",
    );
    map.insert("help.init", "Instructions for init: Detect rooms from a directory and propose a palace structure.");
    map.insert("help.search", "Instructions for search: Find memories by exact or semantic query.");
    map.insert("help.mine", "Instructions for mine: Import files into the palace.");
    map.insert("help.status", "Instructions for status: Show what has been filed.");
    map.insert("terms.palace", "palace");
    map.insert("terms.wing", "wing");
    map.insert("terms.hall", "hall");
    map.insert("terms.closet", "closet");
    map.insert("terms.drawer", "drawer");
    map.insert("terms.mine", "mine");
    map.insert("terms.search", "search");
    map.insert("terms.status", "status");
    map.insert("terms.init", "init");
    map.insert("terms.repair", "repair");
    map.insert("terms.entity", "entity");
    map.insert("terms.topic", "topic");
    map.insert("cli.mine_start", "Mining {path}...");
    map.insert(
        "cli.mine_complete",
        "Done. {closets} closets, {drawers} drawers created.",
    );
    map.insert("cli.mine_skip", "Already mined. Use --force to re-mine.");
    map.insert("cli.search_no_results", "No results for: {query}");
    map.insert("cli.search_results", "Found {count} results:");
    map.insert("cli.status_palace", "Palace: {path}");
    map.insert("cli.status_wings", "{count} wings");
    map.insert("cli.status_closets", "{count} closets");
    map.insert("cli.status_drawers", "{count} drawers");
    map.insert("cli.init_complete", "Palace initialized at {path}");
    map.insert("cli.init_exists", "Palace already exists at {path}");
    map.insert(
        "cli.repair_complete",
        "Repair complete. {fixed} issues fixed.",
    );
    map.insert("cli.migrate_complete", "Migration complete.");
    map.insert(
        "cli.no_palace",
        "No palace found. Run: mempalace init <dir>",
    );
    map.insert(
        "onboarding.welcome",
        "Welcome to MemPalace! Let's set up your memory palace.",
    );
    map.insert("onboarding.mode_prompt", "Choose your primary Mode");
    map.insert(
        "onboarding.people_prompt",
        "Who are the key People in your life? (comma separated)",
    );
    map.insert(
        "onboarding.projects_prompt",
        "What Projects are you currently working on? (comma separated)",
    );
    map.insert(
        "onboarding.wings_prompt",
        "Any specific Wings (categories) you want to track? (comma separated)",
    );
    map.insert(
        "onboarding.bootstrap",
        "Great! Bootstrapping your memory...",
    );
    map.insert(
        "onboarding.origin",
        "Detected onboarding corpus origin: {origin} (confidence: {confidence:.2})",
    );
    map.insert(
        "onboarding.complete",
        "Onboarding complete! Your palace is ready.",
    );
    map.insert("plural.drawer", "drawer");
    map.insert("plural.drawers", "drawers");
    map.insert("aaak.instruction", "Compress to index format. Hyphens between words, pipes between concepts. Drop articles and filler. Keep names and numbers exact.");
    TranslationSet { map }
}

fn build_es() -> TranslationSet {
    let mut map = HashMap::new();
    map.insert("greeting", "Bienvenido a MemPalace!");
    map.insert("greeting_named", "Bienvenido a MemPalace, {name}!");
    map.insert(
        "help",
        "MemPalace — memoria de IA offline-first\n\
        \n\
        Uso: mempalace <COMANDO>\n\
        \n\
        Comandos comunes:\n\
          init <dir>        Detectar salas de la estructura de carpetas\n\
          mine <dir>        Extraer archivos en el palacio\n\
          search <consulta> Buscar con palabras exactas o semánticas\n\
          status            Mostrar lo archivado\n\
          repair            Reindexar entradas en almacenamiento vectorial\n\
          instructions      Imprimir prompts de sistema para agentes IA\n\
        \n\
        Use MEMPALACE_LANG para cambiar el idioma (p. ej. en, es, fr).",
    );
    map.insert("terms.palace", "palacio");
    map.insert("terms.wing", "ala");
    map.insert("terms.hall", "pasillo");
    map.insert("terms.closet", "armario");
    map.insert("terms.drawer", "cajón");
    map.insert("terms.mine", "extraer");
    map.insert("terms.search", "buscar");
    map.insert("terms.status", "estado");
    map.insert("terms.init", "init");
    map.insert("terms.repair", "reparar");
    map.insert("terms.entity", "entidad");
    map.insert("terms.topic", "tema");
    map.insert("cli.mine_start", "Extrayendo {path}...");
    map.insert(
        "cli.mine_complete",
        "Listo. {closets} armarios, {drawers} cajones creados.",
    );
    map.insert("cli.mine_skip", "Ya extraído. Use --force para re-extraer.");
    map.insert("cli.search_no_results", "Sin resultados para: {query}");
    map.insert("cli.search_results", "Se encontraron {count} resultados:");
    map.insert("cli.status_palace", "Palacio: {path}");
    map.insert("cli.status_wings", "{count} alas");
    map.insert("cli.status_closets", "{count} armarios");
    map.insert("cli.status_drawers", "{count} cajones");
    map.insert("cli.init_complete", "Palacio inicializado en {path}");
    map.insert("cli.init_exists", "El palacio ya existe en {path}");
    map.insert(
        "cli.repair_complete",
        "Reparación completa. {fixed} problemas corregidos.",
    );
    map.insert("cli.migrate_complete", "Migración completa.");
    map.insert(
        "cli.no_palace",
        "No se encontró palacio. Ejecute: mempalace init <dir>",
    );
    map.insert(
        "onboarding.welcome",
        "Bienvenido a MemPalace! Configuremos su palacio de memoria.",
    );
    map.insert("onboarding.mode_prompt", "Elija su modo principal");
    map.insert(
        "onboarding.people_prompt",
        "¿Quiénes son las personas clave en su vida? (separados por coma)",
    );
    map.insert(
        "onboarding.projects_prompt",
        "¿En qué proyectos está trabajando? (separados por coma)",
    );
    map.insert(
        "onboarding.wings_prompt",
        "¿Alguna Ala (categoría) específica que desee seguir? (separados por coma)",
    );
    map.insert(
        "onboarding.bootstrap",
        "¡Perfecto! Preparando su memoria...",
    );
    map.insert(
        "onboarding.origin",
        "Origen del corpus de onboarding detectado: {origin} (confianza: {confidence:.2})",
    );
    map.insert(
        "onboarding.complete",
        "¡Onboarding completo! Su palacio está listo.",
    );
    map.insert("plural.drawer", "cajón");
    map.insert("plural.drawers", "cajones");
    map.insert("aaak.instruction", "Comprimir al formato índice. Guiones entre palabras, barras entre conceptos. Omitir artículos y relleno. Mantener nombres y números exactos.");
    TranslationSet { map }
}

fn build_fr() -> TranslationSet {
    let mut map = HashMap::new();
    map.insert("greeting", "Bienvenue sur MemPalace!");
    map.insert("greeting_named", "Bienvenue sur MemPalace, {name}!");
    map.insert(
        "help",
        "MemPalace — mémoire IA offline-first\n\
        \n\
        Utilisation : mempalace <COMMANDE>\n\
        \n\
        Commandes courantes :\n\
          init <dir>        Détecter les salles depuis la structure de dossiers\n\
          mine <dir>        Extraire les fichiers dans le palais\n\
          search <requête>  Rechercher par mots exacts ou sémantique\n\
          status            Afficher ce qui a été archivé\n\
          repair            Réindexer les entrées dans le stockage vectoriel\n\
          instructions      Afficher l'aide localisée des commandes\n\
        \n\
        Définissez MEMPALACE_LANG pour changer la langue (ex. en, es, fr).",
    );
    map.insert("terms.wing", "aile");
    map.insert("terms.closet", "placard");
    map.insert("terms.drawer", "tiroir");
    map.insert("terms.search", "rechercher");
    map.insert("terms.status", "statut");
    map.insert("cli.mine_start", "Extraction de {path}...");
    map.insert("cli.search_no_results", "Aucun résultat pour : {query}");
    map.insert("cli.search_results", "{count} résultats trouvés :");
    map.insert("cli.init_complete", "Palais initialisé à {path}");
    map.insert(
        "cli.no_palace",
        "Aucun palais trouvé. Exécutez : mempalace init <dir>",
    );
    map.insert(
        "onboarding.welcome",
        "Bienvenue sur MemPalace ! Configurons votre palais de mémoire.",
    );
    map.insert("plural.drawer", "tiroir");
    map.insert("plural.drawers", "tiroirs");
    map.insert("aaak.instruction", "Compresser au format index. Traits d'union entre les mots, barres entre les concepts. Supprimer les articles et le remplissage. Garder les noms et les chiffres exacts.");
    TranslationSet { map }
}

fn build_de() -> TranslationSet {
    let mut map = HashMap::new();
    map.insert("greeting", "Willkommen bei MemPalace!");
    map.insert("greeting_named", "Willkommen bei MemPalace, {name}!");
    map.insert(
        "help",
        "MemPalace — offline-first KI-Gedächtnis\n\
        \n\
        Verwendung: mempalace <BEFEHL>\n\
        \n\
        Häufige Befehle:\n\
          init <dir>        Räume aus der Ordnerstruktur erkennen\n\
          mine <dir>        Dateien in den Palast extrahieren\n\
          search <abfrage>  Mit exakten oder semantischen Begriffen suchen\n\
          status            Anzeigen, was archiviert wurde\n\
          repair            Einträge in den Vektorspeicher reindizieren\n\
          instructions      Lokalisierte Befehlshilfe anzeigen\n\
        \n\
        Setzen Sie MEMPALACE_LANG, um die Sprache zu ändern (z. B. en, es, de).",
    );
    map.insert("terms.wing", "Flügel");
    map.insert("terms.closet", "Schrank");
    map.insert("terms.drawer", "Schublade");
    map.insert("terms.search", "suchen");
    map.insert("terms.status", "status");
    map.insert("cli.mine_start", "Mining {path}...");
    map.insert("cli.search_no_results", "Keine Ergebnisse für: {query}");
    map.insert("cli.search_results", "{count} Ergebnisse gefunden:");
    map.insert("cli.init_complete", "Palast initialisiert unter {path}");
    map.insert(
        "cli.no_palace",
        "Kein Palast gefunden. Führen Sie aus: mempalace init <dir>",
    );
    map.insert(
        "onboarding.welcome",
        "Willkommen bei MemPalace! Richten wir Ihren Gedächtnispalast ein.",
    );
    map.insert("plural.drawer", "Schublade");
    map.insert("plural.drawers", "Schubladen");
    map.insert("aaak.instruction", "In Indexformat komprimieren. Bindestriche zwischen Wörtern, Pipes zwischen Konzepten. Artikel und Füllwörter weglassen. Namen und Zahlen exakt beibehalten.");
    TranslationSet { map }
}

fn build_pt_br() -> TranslationSet {
    let mut map = HashMap::new();
    map.insert("greeting", "Bem-vindo ao MemPalace!");
    map.insert("greeting_named", "Bem-vindo ao MemPalace, {name}!");
    map.insert("terms.wing", "ala");
    map.insert("terms.drawer", "gaveta");
    map.insert("cli.search_no_results", "Nenhum resultado para: {query}");
    map.insert(
        "onboarding.welcome",
        "Bem-vindo ao MemPalace! Vamos configurar seu palácio de memória.",
    );
    map.insert("plural.drawer", "gaveta");
    map.insert("plural.drawers", "gavetas");
    TranslationSet { map }
}

fn build_zh_cn() -> TranslationSet {
    let mut map = HashMap::new();
    map.insert("greeting", "欢迎使用 MemPalace!");
    map.insert("greeting_named", "{name}，欢迎使用 MemPalace!");
    map.insert("terms.wing", "翼");
    map.insert("terms.drawer", "抽屉");
    map.insert("cli.search_no_results", "未找到 {query} 的结果");
    map.insert(
        "onboarding.welcome",
        "欢迎使用 MemPalace！让我们设置您的记忆宫殿。",
    );
    map.insert("plural.drawer", "抽屉");
    map.insert("plural.drawers", "抽屉");
    TranslationSet { map }
}

fn build_ja() -> TranslationSet {
    let mut map = HashMap::new();
    map.insert("greeting", "MemPalace へようこそ!");
    map.insert("greeting_named", "{name} さん、MemPalace へようこそ!");
    map.insert("terms.wing", "翼");
    map.insert("terms.drawer", "引き出し");
    map.insert("cli.search_no_results", "{query} の検索結果はありません");
    map.insert(
        "onboarding.welcome",
        "MemPalace へようこそ！記憶の宮殿を設定しましょう。",
    );
    map.insert("plural.drawer", "引き出し");
    map.insert("plural.drawers", "引き出し");
    TranslationSet { map }
}

fn build_ru() -> TranslationSet {
    let mut map = HashMap::new();
    map.insert("greeting", "Добро пожаловать в MemPalace!");
    map.insert("greeting_named", "Добро пожаловать в MemPalace, {name}!");
    map.insert("terms.wing", "крыло");
    map.insert("terms.drawer", "ящик");
    map.insert("cli.search_no_results", "Нет результатов для: {query}");
    map.insert(
        "onboarding.welcome",
        "Добро пожаловать в MemPalace! Настроим ваш дворец памяти.",
    );
    map.insert("plural.drawer", "ящик");
    map.insert("plural.drawers", "ящика");
    TranslationSet { map }
}

fn build_be() -> TranslationSet {
    let mut map = HashMap::new();
    map.insert("greeting", "Сардэчна запрашаем у MemPalace!");
    map.insert("greeting_named", "Сардэчна запрашаем у MemPalace, {name}!");
    map.insert("terms.wing", "крыло");
    map.insert("terms.drawer", "шуфляда");
    map.insert("cli.search_no_results", "Няма вынікаў для: {query}");
    map.insert(
        "onboarding.welcome",
        "Сардэчна запрашаем у MemPalace! Наладзім ваш палац памяці.",
    );
    map.insert("plural.drawer", "шуфляда");
    map.insert("plural.drawers", "шуфляды");
    TranslationSet { map }
}

thread_local! {
    static CURRENT_LOCALE: std::cell::Cell<Locale> = const { std::cell::Cell::new(Locale::En) };
}

/// Set the current thread's active locale.
pub fn set_locale(locale: Locale) {
    CURRENT_LOCALE.with(|c| c.set(locale));
}

/// Get the current thread's active locale.
pub fn current_locale() -> Locale {
    CURRENT_LOCALE.with(|c| c.get())
}

/// Run a closure with a specific locale, restoring the previous locale afterwards.
pub fn with_locale<R>(locale: Locale, f: impl FnOnce() -> R) -> R {
    let previous = current_locale();
    set_locale(locale);
    let result = f();
    set_locale(previous);
    result
}

/// Detect the user's preferred locale from environment variables.
///
/// Checks, in order:
/// 1. `MEMPALACE_LANG`
/// 2. `LANGUAGE`
/// 3. `LC_ALL`
/// 4. `LC_MESSAGES`
/// 5. `LANG`
///
/// Falls back to English if no supported locale is found.
pub fn detect_locale() -> Locale {
    locale_from_env(&[
        "MEMPALACE_LANG",
        "LANGUAGE",
        "LC_ALL",
        "LC_MESSAGES",
        "LANG",
    ])
}

fn locale_from_env(vars: &[&str]) -> Locale {
    for var in vars {
        if let Ok(val) = std::env::var(var) {
            if let Some(locale) = Locale::from_code(&val) {
                return locale;
            }
        }
    }
    Locale::En
}

/// Simple `{var}` interpolation.
fn interpolate(template: &str, vars: &[(&str, &str)]) -> String {
    let mut result = template.to_string();
    for (key, value) in vars {
        result = result.replace(&format!("{{{}}}", key), value);
    }
    result
}

/// Translation instance bound to a specific locale.
#[derive(Debug, Clone, Copy)]
pub struct I18n {
    locale: Locale,
}

impl I18n {
    /// Create a new translator for the given locale.
    pub fn new(locale: Locale) -> Self {
        Self { locale }
    }

    /// Create a translator using the detected system locale.
    pub fn with_detected() -> Self {
        Self::new(detect_locale())
    }

    /// Change the locale for this instance.
    pub fn set_locale(&mut self, locale: Locale) {
        self.locale = locale;
    }

    /// Return the locale this instance is using.
    pub fn locale(&self) -> Locale {
        self.locale
    }

    /// Look up a translation by dotted key.
    ///
    /// Falls back to English if the key is missing for the current locale,
    /// then returns the key itself as a last resort.
    pub fn t(&self, key: &str) -> String {
        TRANSLATIONS
            .get(&self.locale)
            .and_then(|ts| ts.get(key))
            .or_else(|| TRANSLATIONS.get(&Locale::En).and_then(|ts| ts.get(key)))
            .map(|s| s.to_string())
            .unwrap_or_else(|| key.to_string())
    }

    /// Look up a translation and interpolate `{var}` placeholders.
    pub fn tf(&self, key: &str, vars: &[(&str, &str)]) -> String {
        interpolate(&self.t(key), vars)
    }

    /// Return a localized greeting.
    ///
    /// If `name` is provided, a personalized greeting is returned.
    pub fn greeting(&self, name: Option<&str>) -> String {
        match name {
            Some(n) => self.tf("greeting_named", &[("name", n)]),
            None => self.t("greeting"),
        }
    }

    /// Return the localized help text.
    pub fn help(&self) -> String {
        self.t("help")
    }

    /// Return a localized singular or plural form based on `count`.
    ///
    /// This is a simple one/other helper. Languages with more complex
    /// plural rules (e.g. Russian) should extend this in the future.
    pub fn pluralize(&self, count: usize, singular_key: &str, plural_key: &str) -> String {
        let key = if count == 1 { singular_key } else { plural_key };
        self.t(key)
    }

    /// Count the number of drawers with a localized label.
    pub fn drawers_count(&self, count: usize) -> String {
        format!(
            "{} {}",
            count,
            self.pluralize(count, "plural.drawer", "plural.drawers")
        )
    }
}

impl Default for I18n {
    fn default() -> Self {
        Self::new(Locale::default())
    }
}

/// Look up a translation using the current thread locale.
pub fn t(key: &str) -> String {
    I18n::new(current_locale()).t(key)
}

/// Look up and interpolate a translation using the current thread locale.
pub fn tf(key: &str, vars: &[(&str, &str)]) -> String {
    I18n::new(current_locale()).tf(key, vars)
}

/// Return a localized greeting using the current thread locale.
pub fn greeting(name: Option<&str>) -> String {
    I18n::new(current_locale()).greeting(name)
}

/// Return the localized help text using the current thread locale.
pub fn help() -> String {
    I18n::new(current_locale()).help()
}

/// Return localized help/instructions for a specific command.
///
/// Looks up the key `help.{command}` (e.g. `help.init`). Falls back to the
/// English translation and then to the key itself if no translation exists.
pub fn help_for(command: &str) -> String {
    t(&format!("help.{command}"))
}

/// Simple pluralization helper using the current thread locale.
pub fn pluralize(count: usize, singular_key: &str, plural_key: &str) -> String {
    I18n::new(current_locale()).pluralize(count, singular_key, plural_key)
}

/// Return the list of supported locale codes.
pub fn available_languages() -> Vec<&'static str> {
    let mut langs: Vec<&'static str> = TRANSLATIONS.keys().map(|l| l.as_str()).collect();
    langs.sort_unstable();
    langs
}

/// Translate-format convenience macro.
///
/// ```ignore
/// let s = t!("cli.search_results", count => "5");
/// ```
#[macro_export]
macro_rules! t {
    ($key:expr) => {
        $crate::i18n::t($key)
    };
    ($key:expr, $($var:expr => $val:expr),+ $(,)?) => {
        $crate::i18n::tf($key, &[$(($var, $val)),+])
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Serialize tests that mutate process environment variables.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn locale_as_str() {
        assert_eq!(Locale::En.as_str(), "en");
        assert_eq!(Locale::PtBr.as_str(), "pt-BR");
        assert_eq!(Locale::ZhCn.as_str(), "zh-CN");
    }

    #[test]
    fn locale_from_code() {
        assert_eq!(Locale::from_code("en"), Some(Locale::En));
        assert_eq!(Locale::from_code("en-US"), Some(Locale::En));
        assert_eq!(Locale::from_code("en_US"), Some(Locale::En));
        assert_eq!(Locale::from_code("ES"), Some(Locale::Es));
        assert_eq!(Locale::from_code("pt-BR"), Some(Locale::PtBr));
        assert_eq!(Locale::from_code("zh-CN"), Some(Locale::ZhCn));
        assert_eq!(Locale::from_code("zh-TW"), Some(Locale::ZhCn));
        assert_eq!(Locale::from_code("ja"), Some(Locale::Ja));
        assert_eq!(Locale::from_code("ru"), Some(Locale::Ru));
        assert_eq!(Locale::from_code("be"), Some(Locale::Be));
        assert_eq!(Locale::from_code("invalid"), None);
        assert_eq!(Locale::from_code(""), None);
        assert_eq!(Locale::from_code("  en  "), Some(Locale::En));
    }

    #[test]
    fn locale_default() {
        assert_eq!(Locale::default(), Locale::En);
        assert_eq!(Locale::default_locale(), Locale::En);
    }

    #[test]
    fn detect_locale_defaults_to_english() {
        let _guard = ENV_LOCK.lock().unwrap();
        for var in [
            "MEMPALACE_LANG",
            "LANGUAGE",
            "LC_ALL",
            "LC_MESSAGES",
            "LANG",
        ] {
            std::env::remove_var(var);
        }
        assert_eq!(detect_locale(), Locale::En);
    }

    #[test]
    fn detect_locale_from_mempalace_lang() {
        let _guard = ENV_LOCK.lock().unwrap();
        for var in [
            "MEMPALACE_LANG",
            "LANGUAGE",
            "LC_ALL",
            "LC_MESSAGES",
            "LANG",
        ] {
            std::env::remove_var(var);
        }
        std::env::set_var("MEMPALACE_LANG", "es");
        assert_eq!(detect_locale(), Locale::Es);
        std::env::set_var("MEMPALACE_LANG", "fr_FR");
        assert_eq!(detect_locale(), Locale::Fr);
    }

    #[test]
    fn detect_locale_precedence() {
        let _guard = ENV_LOCK.lock().unwrap();
        for var in [
            "MEMPALACE_LANG",
            "LANGUAGE",
            "LC_ALL",
            "LC_MESSAGES",
            "LANG",
        ] {
            std::env::remove_var(var);
        }
        std::env::set_var("LANG", "de");
        std::env::set_var("LC_ALL", "fr");
        assert_eq!(detect_locale(), Locale::Fr);
    }

    #[test]
    fn i18n_new_and_set_locale() {
        let mut i18n = I18n::new(Locale::En);
        assert_eq!(i18n.locale(), Locale::En);
        i18n.set_locale(Locale::Es);
        assert_eq!(i18n.locale(), Locale::Es);
    }

    #[test]
    fn i18n_default() {
        let i18n = I18n::default();
        assert_eq!(i18n.locale(), Locale::En);
    }

    #[test]
    fn translation_lookup() {
        let en = I18n::new(Locale::En);
        let es = I18n::new(Locale::Es);
        assert_eq!(en.t("terms.wing"), "wing");
        assert_eq!(es.t("terms.wing"), "ala");
        assert_eq!(en.t("cli.search_results"), "Found {count} results:");
        assert_eq!(
            es.t("cli.search_results"),
            "Se encontraron {count} resultados:"
        );
    }

    #[test]
    fn translation_fallback() {
        let de = I18n::new(Locale::De);
        // Missing in German -> falls back to English.
        assert_eq!(de.t("terms.palace"), "palace");
        // Unknown key -> returns key itself.
        assert_eq!(de.t("unknown.key"), "unknown.key");
    }

    #[test]
    fn translation_interpolation() {
        let en = I18n::new(Locale::En);
        assert_eq!(
            en.tf("cli.search_results", &[("count", "3")]),
            "Found 3 results:"
        );
        assert_eq!(
            en.tf("cli.mine_complete", &[("closets", "2"), ("drawers", "10")]),
            "Done. 2 closets, 10 drawers created."
        );
        assert_eq!(en.tf("terms.wing", &[]), "wing");
    }

    #[test]
    fn translation_interpolation_missing_var() {
        let en = I18n::new(Locale::En);
        // Missing variable should leave the placeholder intact.
        assert_eq!(en.tf("cli.search_results", &[]), "Found {count} results:");
    }

    #[test]
    fn greeting_lookup() {
        let en = I18n::new(Locale::En);
        let es = I18n::new(Locale::Es);
        assert_eq!(en.greeting(None), "Welcome to MemPalace!");
        assert_eq!(en.greeting(Some("Alice")), "Welcome to MemPalace, Alice!");
        assert_eq!(es.greeting(None), "Bienvenido a MemPalace!");
        assert_eq!(es.greeting(Some("Bob")), "Bienvenido a MemPalace, Bob!");
    }

    #[test]
    fn help_text() {
        let en = I18n::new(Locale::En);
        let text = en.help();
        assert!(text.contains("MemPalace"));
        assert!(text.contains("init <dir>"));
        assert!(text.contains("MEMPALACE_LANG"));
    }

    #[test]
    fn pluralize() {
        let en = I18n::new(Locale::En);
        assert_eq!(en.pluralize(1, "plural.drawer", "plural.drawers"), "drawer");
        assert_eq!(
            en.pluralize(2, "plural.drawer", "plural.drawers"),
            "drawers"
        );
        assert_eq!(en.drawers_count(1), "1 drawer");
        assert_eq!(en.drawers_count(5), "5 drawers");
    }

    #[test]
    fn available_languages_lookup() {
        let langs = available_languages();
        assert!(langs.contains(&"en"));
        assert!(langs.contains(&"es"));
        assert!(langs.contains(&"fr"));
        assert!(langs.contains(&"de"));
    }

    #[test]
    fn i18n_with_detected() {
        let _guard = ENV_LOCK.lock().unwrap();
        for var in [
            "MEMPALACE_LANG",
            "LANGUAGE",
            "LC_ALL",
            "LC_MESSAGES",
            "LANG",
        ] {
            std::env::remove_var(var);
        }
        std::env::set_var("MEMPALACE_LANG", "de");
        let i18n = I18n::with_detected();
        assert_eq!(i18n.locale(), Locale::De);
        assert_eq!(i18n.t("terms.drawer"), "Schublade");
    }

    #[test]
    fn global_locale_functions() {
        with_locale(Locale::En, || {
            assert_eq!(current_locale(), Locale::En);
            assert_eq!(t("terms.wing"), "wing");
            assert_eq!(
                tf("cli.search_results", &[("count", "3")]),
                "Found 3 results:"
            );
            set_locale(Locale::Es);
            assert_eq!(current_locale(), Locale::Es);
            assert_eq!(t("terms.wing"), "ala");
            assert_eq!(greeting(None), "Bienvenido a MemPalace!");
            assert!(help().to_lowercase().contains("mempalace"));
        });
        // Locale is restored after with_locale.
        assert_eq!(current_locale(), Locale::En);
    }

    #[test]
    fn macro_t() {
        with_locale(Locale::En, || {
            assert_eq!(t!("terms.wing"), "wing");
            assert_eq!(t!("cli.search_results", "count" => "7"), "Found 7 results:");
        });
    }

    #[test]
    fn all_locales_have_greeting_and_help() {
        for locale in available_languages() {
            let locale = Locale::from_code(locale).unwrap();
            let i18n = I18n::new(locale);
            assert!(!i18n.greeting(None).is_empty());
            assert!(!i18n.help().is_empty());
        }
    }
}
