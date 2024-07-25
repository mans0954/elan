//! This provides wrappers around the `StdoutTerminal` and `StderrTerminal` types
//! that does not fail if `StdoutTerminal` etc can't be constructed, which happens
//! if TERM isn't defined.

use elan_utils::tty;
use std::io;
use term;
use pulldown_cmark::{Event, Tag};

pub use term::color;
pub use term::Attr;

pub trait Instantiable {
    fn instance() -> Self;
}

impl Instantiable for io::Stdout {
    fn instance() -> Self {
        io::stdout()
    }
}

impl Instantiable for io::Stderr {
    fn instance() -> Self {
        io::stderr()
    }
}

pub trait Isatty {
    fn isatty() -> bool;
}

impl Isatty for io::Stdout {
    fn isatty() -> bool {
        tty::stdout_isatty()
    }
}

impl Isatty for io::Stderr {
    fn isatty() -> bool {
        tty::stderr_isatty()
    }
}

pub struct Terminal<T>(Option<Box<dyn term::Terminal<Output = T> + Send>>)
where
    T: Instantiable + Isatty + io::Write;
pub type StdoutTerminal = Terminal<io::Stdout>;
pub type StderrTerminal = Terminal<io::Stderr>;

pub fn stdout() -> StdoutTerminal {
    Terminal(term::stdout())
}

pub fn stderr() -> StderrTerminal {
    Terminal(term::stderr())
}

// Handles the wrapping of text written to the console
struct LineWrapper<'a, T: io::Write + 'a> {
    indent: u32,
    margin: u32,
    pos: u32,
    pub w: &'a mut T,
}

impl<'a, T: io::Write + 'a> LineWrapper<'a, T> {
    // Just write a newline
    fn write_line(&mut self) {
        let _ = writeln!(self.w, "");
        // Reset column position to start of line
        self.pos = 0;
    }
    // Called before writing text to ensure indent is applied
    fn write_indent(&mut self) {
        if self.pos == 0 {
            // Write a space for each level of indent
            for _ in 0..self.indent {
                let _ = write!(self.w, " ");
            }
            self.pos = self.indent;
        }
    }
    // Write a non-breaking word
    fn write_word(&mut self, word: &str) {
        // Ensure correct indentation
        self.write_indent();
        let word_len = word.len() as u32;

        // If this word goes past the margin
        if self.pos + word_len > self.margin {
            // And adding a newline would give us more space
            if self.pos > self.indent {
                // Then add a newline!
                self.write_line();
                self.write_indent();
            }
        }

        // Write the word
        let _ = write!(self.w, "{}", word);
        self.pos += word_len;
    }
    fn write_space(&mut self) {
        if self.pos > self.indent {
            if self.pos < self.margin {
                self.write_word(" ");
            } else {
                self.write_line();
            }
        }
    }
    // Writes a span of text which wraps at the margin
    fn write_span(&mut self, text: &str) {
        // Allow words to wrap on whitespace
        let mut is_first = true;
        for word in text.split(char::is_whitespace) {
            if is_first {
                is_first = false;
            } else {
                self.write_space();
            }
            self.write_word(word);
        }
    }
    // Constructor
    fn new(w: &'a mut T, indent: u32, margin: u32) -> Self {
        LineWrapper {
            indent: indent,
            margin: margin,
            pos: indent,
            w: w,
        }
    }
}

// Handles the formatting of text
struct LineFormatter<'a, T: Instantiable + Isatty + io::Write + 'a> {
    is_code_block: bool,
    wrapper: LineWrapper<'a, Terminal<T>>,
    attrs: Vec<Attr>,
}

impl<'a, T: Instantiable + Isatty + io::Write + 'a> LineFormatter<'a, T> {
    fn new(w: &'a mut Terminal<T>, indent: u32, margin: u32) -> Self {
        LineFormatter {
            is_code_block: false,
            wrapper: LineWrapper::new(w, indent, margin),
            attrs: Vec::new(),
        }
    }
    fn push_attr(&mut self, attr: Attr) {
        self.attrs.push(attr);
        let _ = self.wrapper.w.attr(attr);
    }
    fn pop_attr(&mut self) {
        self.attrs.pop();
        let _ = self.wrapper.w.reset();
        for attr in &self.attrs {
            let _ = self.wrapper.w.attr(*attr);
        }
    }

    fn start_tag(&mut self, tag: Tag<'a>) {
        match tag {
            Tag::Paragraph => {
                self.wrapper.write_line();
            }

            Tag::Heading(_level, _identifier, _classes) => {
                self.push_attr(Attr::Bold);
                self.wrapper.write_line();
            }
            Tag::Table(_alignments) => {}
            Tag::TableHead => {}
            Tag::TableRow => {}
            Tag::TableCell => {}
            Tag::BlockQuote => {}
            Tag::CodeBlock(_lang) => {
                self.wrapper.write_line();
                self.wrapper.indent += 2;
                self.is_code_block = true;
            }
            Tag::List(_) => {
                self.wrapper.write_line();
                self.wrapper.indent += 2;
            }
            Tag::Item => {
                self.wrapper.write_line();
            }
            Tag::Emphasis => {
                self.push_attr(Attr::ForegroundColor(color::BRIGHT_RED));
            }
            Tag::Strong => {}
            Tag::Strikethrough => {}
            Tag::Link(_link_type, _dest, _title) => {}
            Tag::Image(_link_type, _dest, _title) => {}
            Tag::FootnoteDefinition(_name) => {}
        }
    }

    fn end_tag(&mut self, tag: Tag<'a>) {
        match tag {
            Tag::Paragraph => {
                self.wrapper.write_line();
            }
            Tag::Heading(_level, _identifier, _classes) => {
                self.wrapper.write_line();
                self.pop_attr();
            }
            Tag::Table(_) => {}
            Tag::TableHead => {}
            Tag::TableRow => {}
            Tag::TableCell => {}
            Tag::BlockQuote => {}
            Tag::CodeBlock(_) => {
                self.is_code_block = false;
                self.wrapper.indent -= 2;
            }
            Tag::List(_) => {
                self.wrapper.indent -= 2;
                self.wrapper.write_line();
            }
            Tag::Item => {}
            Tag::Emphasis => {
                self.pop_attr();
            }
            Tag::Strong => {}
            Tag::Strikethrough => {}
            Tag::Link(_, _, _) => {}
            Tag::Image(_, _, _) => {} // shouldn't happen, handled in start
            Tag::FootnoteDefinition(_) => {}
        }
    }

    fn process_event(&mut self, event: Event<'a>) {
        use self::Event::*;
        match event {
            Start(tag) => self.start_tag(tag),
            End(tag) => self.end_tag(tag),
            Text(text) => {
                if self.is_code_block {
                    self.wrapper.write_word(&text);
                } else {
                    self.wrapper.write_span(&text);
                }
            }
            Code(code) => {
                self.push_attr(Attr::Bold);
                self.wrapper.write_word(&code);
                self.pop_attr();
            }
            Html(_html) => {}
            SoftBreak => {
                self.wrapper.write_line();
            }
            HardBreak => {
                self.wrapper.write_line();
            }
            Rule => {}
            FootnoteReference(_name) => {}
            TaskListMarker(true) => {}
            TaskListMarker(false) => {}
        }
    }
}

impl<T: Instantiable + Isatty + io::Write> io::Write for Terminal<T> {
    fn write(&mut self, buf: &[u8]) -> Result<usize, io::Error> {
        if let Some(ref mut t) = self.0 {
            t.write(buf)
        } else {
            let mut t = T::instance();
            t.write(buf)
        }
    }

    fn flush(&mut self) -> Result<(), io::Error> {
        if let Some(ref mut t) = self.0 {
            t.flush()
        } else {
            let mut t = T::instance();
            t.flush()
        }
    }
}

impl<T: Instantiable + Isatty + io::Write> Terminal<T> {
    pub fn fg(&mut self, color: color::Color) -> Result<(), term::Error> {
        if !T::isatty() {
            return Ok(());
        }

        if let Some(ref mut t) = self.0 {
            t.fg(color)
        } else {
            Ok(())
        }
    }

    pub fn attr(&mut self, attr: Attr) -> Result<(), term::Error> {
        if !T::isatty() {
            return Ok(());
        }

        if let Some(ref mut t) = self.0 {
            if let Err(e) = t.attr(attr) {
                // If `attr` is not supported, try to emulate it
                match attr {
                    Attr::Bold => t.fg(color::BRIGHT_WHITE),
                    _ => Err(e),
                }
            } else {
                Ok(())
            }
        } else {
            Ok(())
        }
    }

    pub fn reset(&mut self) -> Result<(), term::Error> {
        if !T::isatty() {
            return Ok(());
        }

        if let Some(ref mut t) = self.0 {
            t.reset()
        } else {
            Ok(())
        }
    }

    pub fn md<S: AsRef<str>>(&mut self, content: S) {
        let mut f = LineFormatter::new(self, 0, 79);
        let parser = pulldown_cmark::Parser::new(content.as_ref());
        for event in parser {
            f.process_event(event);
        }
    }
}
