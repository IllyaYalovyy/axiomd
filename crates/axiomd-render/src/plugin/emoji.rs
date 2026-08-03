//! Emoji shortcodes: `:tada:` in the source, 🎉 in the document.
//!
//! The first built-in plugin, and the one that proves the event transform hook: it
//! never touches the pipeline, only the stream of events going through it. What it
//! rewrites is prose — a shortcode inside a code fence or an inline code span is code
//! and stays exactly as it was written.
//!
//! # Where the names come from
//!
//! The shortcodes below are GitHub's, spelled the way GitHub's own `gemoji` database
//! spells them, and each was taken from that database rather than remembered
//! (`github/gemoji`, `db/emoji.json`). The set is the ones documents actually use; a
//! name that is not here is left as the reader wrote it, which is what an unknown
//! shortcode is on GitHub too.
//!
//! Headings keep their anchor ids from the source, not from the emoji: `# :rocket:
//! Launch` is still `#rocket-launch`, so a link written against the document's text
//! survives the plugin being switched on or off.

use std::borrow::Cow;

use axiomd_engine::Event;

use super::{Asset, Manifest, PLUGIN_API, Plugin};

/// The styling a shortcode's replacement needs.
///
/// Only reaches a document that used one, which is what makes it the proof of the
/// conditional-asset path: a document without a shortcode in it does not link this,
/// and neither does any document while the plugin is switched off.
const STYLE: Asset = Asset {
    name: "emoji.css",
    content_type: "text/css",
    bytes: include_bytes!("../../assets/plugin/emoji.css"),
};

const MANIFEST: Manifest = Manifest {
    api: PLUGIN_API,
    id: "emoji",
    name: "Emoji shortcodes",
    description: "Write :tada: and read 🎉, the way GitHub spells them",
    fences: &[],
    assets: &[STYLE],
};

/// The plugin itself, which holds nothing: the table is compiled in.
pub(super) struct Emoji;

impl Plugin for Emoji {
    fn manifest(&self) -> &'static Manifest {
        &MANIFEST
    }

    /// Splits one run of text around the shortcodes in it.
    ///
    /// The emoji is wrapped rather than dropped in as bare text so that it can be
    /// styled: an emoji inside emphasised prose must not be slanted with the words
    /// around it, and that is a rule about a span, not about a character.
    fn rewrite<'a>(&self, event: &Event<'a>) -> Option<Vec<Event<'a>>> {
        let Event::Text(text) = event else {
            return None;
        };
        let text = text.as_ref();
        let mut rewritten: Vec<Event<'a>> = Vec::new();
        let mut written = 0;
        let mut at = 0;
        while let Some(offset) = text[at..].find(':') {
            let start = at + offset;
            let Some((name, end)) = shortcode(text, start) else {
                at = start + 1;
                continue;
            };
            let Some(emoji) = emoji(name) else {
                // The closing colon of a name nobody knows may still open the next
                // shortcode: in `:unknown::tada:` one colon is both.
                at = end - 1;
                continue;
            };
            if written < start {
                rewritten.push(Event::Text(Cow::Owned(text[written..start].to_owned())));
            }
            rewritten.push(Event::InlineHtml(Cow::Borrowed("<span class=\"emoji\">")));
            rewritten.push(Event::Text(Cow::Borrowed(emoji)));
            rewritten.push(Event::InlineHtml(Cow::Borrowed("</span>")));
            written = end;
            at = end;
        }
        if rewritten.is_empty() {
            return None;
        }
        if written < text.len() {
            rewritten.push(Event::Text(Cow::Owned(text[written..].to_owned())));
        }
        Some(rewritten)
    }
}

/// The shortcode opening at `start`, as `(name, end)` where `end` is just past its
/// closing colon — or `None` when what opens there is not one.
fn shortcode(text: &str, start: usize) -> Option<(&str, usize)> {
    let after = &text[start + 1..];
    let length = after.find(':')?;
    let name = &after[..length];
    if name.is_empty() || !name.bytes().all(is_shortcode_byte) {
        return None;
    }
    Some((name, start + 1 + length + 1))
}

/// What a shortcode's name may be spelled with: gemoji's own alphabet, which is
/// lowercase ASCII, digits, `_`, `+` and `-`.
fn is_shortcode_byte(byte: u8) -> bool {
    byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'+' | b'-')
}

/// The emoji `name` stands for, if it stands for one.
fn emoji(name: &str) -> Option<&'static str> {
    EMOJI
        .binary_search_by(|(shortcode, _)| (*shortcode).cmp(name))
        .ok()
        .map(|at| EMOJI[at].1)
}

/// Every shortcode this plugin knows, sorted by name so the lookup is a search rather
/// than a scan.
const EMOJI: &[(&str, &str)] = &[
    ("+1", "👍"),
    ("-1", "👎"),
    ("100", "💯"),
    ("alarm_clock", "⏰"),
    ("alien", "👽"),
    ("angel", "👼"),
    ("angry", "😠"),
    ("ant", "🐜"),
    ("arrow_down", "⬇️"),
    ("arrow_left", "⬅️"),
    ("arrow_right", "➡️"),
    ("arrow_up", "⬆️"),
    ("arrows_counterclockwise", "🔄"),
    ("art", "🎨"),
    ("back", "🔙"),
    ("balance_scale", "⚖️"),
    ("balloon", "🎈"),
    ("bar_chart", "📊"),
    ("battery", "🔋"),
    ("beach_umbrella", "🏖️"),
    ("bear", "🐻"),
    ("bee", "🐝"),
    ("beer", "🍺"),
    ("beers", "🍻"),
    ("bell", "🔔"),
    ("bird", "🐦"),
    ("birthday", "🎂"),
    ("black_heart", "🖤"),
    ("blue_heart", "💙"),
    ("blush", "😊"),
    ("bomb", "💣"),
    ("book", "📖"),
    ("bookmark", "🔖"),
    ("bookmark_tabs", "📑"),
    ("books", "📚"),
    ("boom", "💥"),
    ("brain", "🧠"),
    ("broken_heart", "💔"),
    ("bug", "🐛"),
    ("bulb", "💡"),
    ("butterfly", "🦋"),
    ("cactus", "🌵"),
    ("cake", "🍰"),
    ("calendar", "📆"),
    ("camera", "📷"),
    ("camping", "🏕️"),
    ("cat", "🐱"),
    ("cd", "💿"),
    ("chart_with_downwards_trend", "📉"),
    ("chart_with_upwards_trend", "📈"),
    ("checkered_flag", "🏁"),
    ("chicken", "🐔"),
    ("city_sunset", "🌆"),
    ("clap", "👏"),
    ("clapper", "🎬"),
    ("clipboard", "📋"),
    ("cloud", "☁️"),
    ("coffee", "☕"),
    ("compass", "🧭"),
    ("computer", "💻"),
    ("confetti_ball", "🎊"),
    ("confused", "😕"),
    ("construction", "🚧"),
    ("cookie", "🍪"),
    ("cool", "🆒"),
    ("copyright", "©️"),
    ("cow", "🐮"),
    ("crab", "🦀"),
    ("credit_card", "💳"),
    ("crown", "👑"),
    ("cry", "😢"),
    ("crystal_ball", "🔮"),
    ("dart", "🎯"),
    ("desert", "🏜️"),
    ("dna", "🧬"),
    ("dog", "🐶"),
    ("dollar", "💵"),
    ("dolphin", "🐬"),
    ("dragon", "🐉"),
    ("drum", "🥁"),
    ("dvd", "📀"),
    ("ear", "👂"),
    ("earth_africa", "🌍"),
    ("earth_americas", "🌎"),
    ("earth_asia", "🌏"),
    ("electric_plug", "🔌"),
    ("elephant", "🐘"),
    ("end", "🔚"),
    ("envelope", "✉️"),
    ("evergreen_tree", "🌲"),
    ("exclamation", "❗"),
    ("eyes", "👀"),
    ("fast_forward", "⏩"),
    ("file_folder", "📁"),
    ("fire", "🔥"),
    ("fish", "🐟"),
    ("floppy_disk", "💾"),
    ("four_leaf_clover", "🍀"),
    ("fox_face", "🦊"),
    ("free", "🆓"),
    ("frog", "🐸"),
    ("gear", "⚙️"),
    ("gem", "💎"),
    ("ghost", "👻"),
    ("gift", "🎁"),
    ("globe_with_meridians", "🌐"),
    ("green_heart", "💚"),
    ("grinning", "😀"),
    ("guitar", "🎸"),
    ("hammer", "🔨"),
    ("handshake", "🤝"),
    ("headphones", "🎧"),
    ("heart", "❤️"),
    ("heart_eyes", "😍"),
    ("heavy_check_mark", "✔️"),
    ("heavy_minus_sign", "➖"),
    ("heavy_multiplication_x", "✖️"),
    ("heavy_plus_sign", "➕"),
    ("herb", "🌿"),
    ("horse", "🐴"),
    ("hourglass", "⌛"),
    ("hourglass_flowing_sand", "⏳"),
    ("inbox_tray", "📥"),
    ("information_source", "ℹ️"),
    ("jack_o_lantern", "🎃"),
    ("joy", "😂"),
    ("joystick", "🕹️"),
    ("key", "🔑"),
    ("keyboard", "⌨️"),
    ("koala", "🐨"),
    ("label", "🏷️"),
    ("laughing", "😆"),
    ("link", "🔗"),
    ("lion", "🦁"),
    ("lipstick", "💄"),
    ("lock", "🔒"),
    ("mag", "🔍"),
    ("magnet", "🧲"),
    ("mailbox", "📫"),
    ("maple_leaf", "🍁"),
    ("mega", "📣"),
    ("memo", "📝"),
    ("microscope", "🔬"),
    ("milky_way", "🌌"),
    ("moneybag", "💰"),
    ("monkey", "🐒"),
    ("mount_fuji", "🗻"),
    ("movie_camera", "🎥"),
    ("muscle", "💪"),
    ("musical_note", "🎵"),
    ("neutral_face", "😐"),
    ("new", "🆕"),
    ("no_entry", "⛔"),
    ("no_entry_sign", "🚫"),
    ("notes", "🎶"),
    ("octopus", "🐙"),
    ("ok", "🆗"),
    ("ok_hand", "👌"),
    ("on", "🔛"),
    ("open_file_folder", "📂"),
    ("orange_heart", "🧡"),
    ("outbox_tray", "📤"),
    ("owl", "🦉"),
    ("package", "📦"),
    ("page_facing_up", "📄"),
    ("panda_face", "🐼"),
    ("paperclip", "📎"),
    ("pencil", "📝"),
    ("penguin", "🐧"),
    ("pig", "🐷"),
    ("pizza", "🍕"),
    ("point_down", "👇"),
    ("point_left", "👈"),
    ("point_right", "👉"),
    ("point_up", "☝️"),
    ("pray", "🙏"),
    ("printer", "🖨️"),
    ("purple_heart", "💜"),
    ("pushpin", "📌"),
    ("question", "❓"),
    ("rabbit", "🐰"),
    ("rage", "😡"),
    ("rainbow", "🌈"),
    ("raised_hands", "🙌"),
    ("receipt", "🧾"),
    ("record_button", "⏺️"),
    ("recycle", "♻️"),
    ("registered", "®️"),
    ("repeat", "🔁"),
    ("rewind", "⏪"),
    ("ring", "💍"),
    ("robot", "🤖"),
    ("rocket", "🚀"),
    ("rose", "🌹"),
    ("santa", "🎅"),
    ("satellite", "📡"),
    ("scissors", "✂️"),
    ("scroll", "📜"),
    ("seedling", "🌱"),
    ("sheep", "🐑"),
    ("shield", "🛡️"),
    ("skull", "💀"),
    ("smile", "😄"),
    ("smiley", "😃"),
    ("snail", "🐌"),
    ("snake", "🐍"),
    ("snowflake", "❄️"),
    ("snowman", "⛄"),
    ("sob", "😭"),
    ("soon", "🔜"),
    ("sos", "🆘"),
    ("space_invader", "👾"),
    ("sparkles", "✨"),
    ("sparkling_heart", "💖"),
    ("speech_balloon", "💬"),
    ("spider", "🕷️"),
    ("star", "⭐"),
    ("star2", "🌟"),
    ("stop_button", "⏹️"),
    ("stopwatch", "⏱️"),
    ("straight_ruler", "📏"),
    ("sunflower", "🌻"),
    ("sunny", "☀️"),
    ("sunrise", "🌅"),
    ("tada", "🎉"),
    ("tea", "🍵"),
    ("telephone", "☎️"),
    ("telescope", "🔭"),
    ("test_tube", "🧪"),
    ("thinking", "🤔"),
    ("thought_balloon", "💭"),
    ("thumbsdown", "👎"),
    ("thumbsup", "👍"),
    ("tiger", "🐯"),
    ("tm", "™️"),
    ("toolbox", "🧰"),
    ("top", "🔝"),
    ("triangular_flag_on_post", "🚩"),
    ("trophy", "🏆"),
    ("trumpet", "🎺"),
    ("turtle", "🐢"),
    ("umbrella", "☔"),
    ("unicorn", "🦄"),
    ("unlock", "🔓"),
    ("up", "🆙"),
    ("violin", "🎻"),
    ("volcano", "🌋"),
    ("warning", "⚠️"),
    ("watch", "⌚"),
    ("wave", "👋"),
    ("whale", "🐳"),
    ("white_check_mark", "✅"),
    ("white_heart", "🤍"),
    ("wink", "😉"),
    ("world_map", "🗺️"),
    ("wrench", "🔧"),
    ("x", "❌"),
    ("yellow_heart", "💛"),
    ("zap", "⚡"),
];
