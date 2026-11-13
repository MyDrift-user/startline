//! Reads SQLite databases through the copy of SQLite that comes with Windows (winsqlite3.dll), so nothing has to be
//! built or installed for it. Only used on copies of other programs' files: the browsers' bookmarks and their pictures.

use std::ffi::{CString, c_char, c_int, c_void};
use std::sync::OnceLock;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::core::{PCSTR, w};

const OK: c_int = 0;
const ROW: c_int = 100;
const OPEN_READWRITE: c_int = 0x2;
const OPEN_CREATE: c_int = 0x4;

type Open = unsafe extern "system" fn(*const c_char, *mut *mut c_void, c_int, *const c_char) -> c_int;
type Prepare = unsafe extern "system" fn(*mut c_void, *const c_char, c_int, *mut *mut c_void, *mut *const c_char) -> c_int;
type Step = unsafe extern "system" fn(*mut c_void) -> c_int;
type ColumnBlob = unsafe extern "system" fn(*mut c_void, c_int) -> *const u8;
type ColumnText = ColumnBlob;
type ColumnType = unsafe extern "system" fn(*mut c_void, c_int) -> c_int;
type ColumnBytes = unsafe extern "system" fn(*mut c_void, c_int) -> c_int;
type ColumnCount = unsafe extern "system" fn(*mut c_void) -> c_int;
type Finish = unsafe extern "system" fn(*mut c_void) -> c_int;
type Exec = unsafe extern "system" fn(*mut c_void, *const c_char, *const c_void, *mut c_void, *mut *mut c_char) -> c_int;

struct Api {
    open: Open,
    prepare: Prepare,
    step: Step,
    text: ColumnText,
    blob: ColumnBlob,
    kind: ColumnType,
    bytes: ColumnBytes,
    count: ColumnCount,
    finalize: Finish,
    close: Finish,
    #[cfg_attr(not(test), allow(dead_code))]
    exec: Exec,
}

fn api() -> Option<&'static Api> {
    static API: OnceLock<Option<Api>> = OnceLock::new();
    API.get_or_init(|| unsafe {
        let lib = LoadLibraryW(w!("winsqlite3.dll")).ok()?;
        macro_rules! get {
            ($name:literal) => {
                std::mem::transmute(GetProcAddress(lib, PCSTR(concat!($name, "\0").as_ptr()))?)
            };
        }
        Some(Api {
            open: get!("sqlite3_open_v2"),
            prepare: get!("sqlite3_prepare_v2"),
            step: get!("sqlite3_step"),
            text: get!("sqlite3_column_text"),
            blob: get!("sqlite3_column_blob"),
            kind: get!("sqlite3_column_type"),
            bytes: get!("sqlite3_column_bytes"),
            count: get!("sqlite3_column_count"),
            finalize: get!("sqlite3_finalize"),
            close: get!("sqlite3_close"),
            exec: get!("sqlite3_exec"),
        })
    })
    .as_ref()
}

/// An open database. Closed when dropped.
pub struct Db {
    api: &'static Api,
    handle: *mut c_void,
}

impl Db {
    /// Opens a database file, making it if `create`. None when Windows has no SQLite or the file isn't a database.
    pub fn open(path: &std::path::Path, create: bool) -> Option<Db> {
        let api = api()?;
        let name = CString::new(path.to_string_lossy().as_bytes()).ok()?;
        let mut handle = std::ptr::null_mut();
        let flags = OPEN_READWRITE | if create { OPEN_CREATE } else { 0 };
        let status = unsafe { (api.open)(name.as_ptr(), &mut handle, flags, std::ptr::null()) };
        let db = Db { api, handle };
        (status == OK && !handle.is_null()).then_some(db)
    }

    /// Runs statements that return nothing.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn exec(&self, sql: &str) -> bool {
        let Ok(sql) = CString::new(sql) else { return false };
        unsafe { (self.api.exec)(self.handle, sql.as_ptr(), std::ptr::null(), std::ptr::null_mut(), std::ptr::null_mut()) == OK }
    }

    /// Every row of a query, each column as text (None where it's empty in the database).
    pub fn rows(&self, sql: &str) -> Option<Vec<Vec<Option<String>>>> {
        let rows = self.cells(sql)?;
        Some(rows.into_iter().map(|row| row.into_iter().map(|cell| cell.map(|bytes| String::from_utf8_lossy(&bytes).into_owned())).collect()).collect())
    }

    /// Every row of a query, each column as its bytes: a picture, or text in UTF-8 whatever the database keeps it in.
    /// None where it's empty.
    pub fn cells(&self, sql: &str) -> Option<Vec<Vec<Option<Vec<u8>>>>> {
        let sql = CString::new(sql).ok()?;
        let mut statement = std::ptr::null_mut();
        let status = unsafe { (self.api.prepare)(self.handle, sql.as_ptr(), -1, &mut statement, std::ptr::null_mut()) };
        if status != OK || statement.is_null() {
            return None;
        }
        let mut out = Vec::new();
        unsafe {
            let columns = (self.api.count)(statement);
            while (self.api.step)(statement) == ROW {
                let row = (0..columns)
                    .map(|at| {
                        const BLOB: c_int = 4;
                        const NULL: c_int = 5;
                        let kind = (self.api.kind)(statement, at);
                        if kind == NULL {
                            return None;
                        }
                        // Text comes as UTF-8 even from a database that keeps it as UTF-16; a picture as it is.
                        let data = if kind == BLOB { (self.api.blob)(statement, at) } else { (self.api.text)(statement, at) };
                        let len = (self.api.bytes)(statement, at).max(0) as usize;
                        Some(if data.is_null() || len == 0 { Vec::new() } else { std::slice::from_raw_parts(data, len).to_vec() })
                    })
                    .collect();
                out.push(row);
            }
            (self.api.finalize)(statement);
        }
        Some(out)
    }
}

impl Drop for Db {
    fn drop(&mut self) {
        unsafe { (self.api.close)(self.handle) };
    }
}

/// Whether Windows has SQLite here.
#[cfg(test)]
pub fn available() -> bool {
    api().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_read_as_utf8_whatever_the_database_keeps_it_in() {
        if !available() {
            return;
        }
        let path = std::env::temp_dir().join(format!("startline-test-utf16-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let db = Db::open(&path, true).unwrap();
            assert!(db.exec("PRAGMA encoding = 'UTF-16le'; CREATE TABLE t (name TEXT, picture BLOB, n INTEGER); INSERT INTO t VALUES ('Größe', X'0001FF', 7);"));
            assert_eq!(db.rows("SELECT name, n FROM t").unwrap(), vec![vec![Some("Größe".to_string()), Some("7".to_string())]]);
            assert_eq!(db.cells("SELECT picture FROM t").unwrap(), vec![vec![Some(vec![0, 1, 255])]], "a picture keeps its bytes");
        }
        let _ = std::fs::remove_file(&path);
    }
}
