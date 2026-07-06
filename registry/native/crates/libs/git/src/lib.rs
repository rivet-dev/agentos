//! Minimal clean-room git implementation (Apache-2.0).
//!
//! Supports: init, add, commit, branch, checkout (with DWIM), local clone,
//! and smart-HTTP clone over http:// and https://.

use flate2::bufread::ZlibDecoder as BufZlibDecoder;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use sha1::{Digest, Sha1};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::io::{self, Cursor, Read, Write};
use std::path::{Component, Path, PathBuf};
use wasi_http::{HttpClient, Method, Request};

// ─── Hex utilities ──────────────────────────────────────────────────────────

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn unhex(s: &str) -> io::Result<[u8; 20]> {
    if s.len() != 40 {
        return Err(err(&format!("invalid hash length: {}", s.len())));
    }
    let bytes: Vec<u8> = (0..s.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| err(&format!("invalid hex: {}", e)))
        })
        .collect::<io::Result<Vec<u8>>>()?;
    let mut hash = [0u8; 20];
    hash.copy_from_slice(&bytes);
    Ok(hash)
}

fn err(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::Other, msg.to_string())
}

fn print_stdout_line(args: fmt::Arguments<'_>) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    stdout.write_fmt(args)?;
    stdout.write_all(b"\n")?;
    stdout.flush()
}

const SUPPORT_DOC_PATH: &str = "registry/native/crates/libs/git/README.md";
const MAX_GIT_OBJECT_BYTES: usize = 128 * 1024 * 1024;
const MAX_GIT_OBJECT_HEADER_BYTES: usize = 128;
const MAX_INDEX_ENTRIES: usize = 1_000_000;
const MAX_INDEX_BYTES: usize = 256 * 1024 * 1024;
const MAX_ADVERTISED_REFS: usize = 100_000;
const MAX_HTTP_ERROR_BODY_BYTES: usize = 8192;
const MAX_PKT_LINES: usize = 100_000;
const MAX_PKT_LINE_PAYLOAD_BYTES: usize = 16 * 1024 * 1024;
const MAX_REF_ADVERTISEMENT_BYTES: usize = 16 * 1024 * 1024;
const MAX_PACK_INFLATED_BYTES: usize = 256 * 1024 * 1024;
const MAX_PACK_BYTES: usize = 256 * 1024 * 1024;
const MAX_PACK_OBJECTS: usize = 1_000_000;
const MAX_PACK_RESOLVED_BYTES: usize = 256 * 1024 * 1024;

fn unsupported(subcommand: &str, detail: &str) -> io::Error {
    err(&format!(
        "GitSubcommandUnsupported: git {} {} See {} for supported transports and commands.",
        subcommand, detail, SUPPORT_DOC_PATH
    ))
}

/// WASI-safe mkdir -p: create_dir_all has issues with WASI permission checks
/// on already-existing directories, so we create one level at a time.
/// We also treat PermissionDenied as "already exists" because some WASI runtimes
/// return EACCES instead of EEXIST for existing directories.
fn mkdirs(path: &Path) -> io::Result<()> {
    let mut ancestors: Vec<&Path> = path.ancestors().collect();
    ancestors.reverse(); // root first
    for dir in ancestors {
        if dir == Path::new("/") || dir == Path::new("") {
            continue;
        }
        match fs::create_dir(dir) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            // WASI kernels may return EACCES for existing directories
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
                // Verify it actually exists; propagate if it truly doesn't
                if !dir.exists() {
                    return Err(e);
                }
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

fn dir_is_empty(path: &Path) -> io::Result<bool> {
    if !path.exists() {
        return Ok(true);
    }
    if !path.is_dir() {
        return Ok(false);
    }
    Ok(fs::read_dir(path)?.next().is_none())
}

fn read_to_end_limited<R: Read>(reader: R, limit: usize, context: &str) -> io::Result<Vec<u8>> {
    let limit_plus_one = limit
        .checked_add(1)
        .ok_or_else(|| err(&format!("{} size limit is too large", context)))?;
    let mut out = Vec::new();
    reader.take(limit_plus_one as u64).read_to_end(&mut out)?;
    if out.len() > limit {
        return Err(err(&format!("{} exceeds size limit", context)));
    }
    Ok(out)
}

fn read_file_limited(path: &Path, limit: usize, context: &str) -> io::Result<Vec<u8>> {
    let metadata = fs::metadata(path).map_err(|e| {
        err(&format!(
            "cannot stat {} '{}': {}",
            context,
            path.display(),
            e
        ))
    })?;
    if metadata.len() > limit as u64 {
        return Err(err(&format!(
            "{} '{}' exceeds size limit",
            context,
            path.display()
        )));
    }
    fs::read(path).map_err(|e| {
        err(&format!(
            "cannot read {} '{}': {}",
            context,
            path.display(),
            e
        ))
    })
}

fn try_reserve_exact<T>(vec: &mut Vec<T>, additional: usize, context: &str) -> io::Result<()> {
    vec.try_reserve_exact(additional)
        .map_err(|_| err(&format!("{} allocation failed", context)))
}

fn append_pack_bytes(pack: &mut Vec<u8>, bytes: &[u8]) -> io::Result<()> {
    let new_len = pack
        .len()
        .checked_add(bytes.len())
        .ok_or_else(|| err("packfile size overflow"))?;
    if new_len > MAX_PACK_BYTES {
        return Err(err("packfile exceeds size limit"));
    }
    try_reserve_exact(pack, bytes.len(), "packfile")?;
    pack.extend_from_slice(bytes);
    Ok(())
}

fn add_pack_inflated_bytes(total: &mut usize, bytes: usize) -> io::Result<()> {
    *total = total
        .checked_add(bytes)
        .ok_or_else(|| err("inflated pack size overflow"))?;
    if *total > MAX_PACK_INFLATED_BYTES {
        return Err(err("inflated pack data exceeds size limit"));
    }
    Ok(())
}

fn add_pack_resolved_bytes(total: &mut usize, bytes: usize) -> io::Result<()> {
    *total = total
        .checked_add(bytes)
        .ok_or_else(|| err("resolved pack size overflow"))?;
    if *total > MAX_PACK_RESOLVED_BYTES {
        return Err(err("resolved pack data exceeds size limit"));
    }
    Ok(())
}

fn add_pkt_payload_bytes(total: &mut usize, bytes: usize) -> io::Result<()> {
    *total = total
        .checked_add(bytes)
        .ok_or_else(|| err("pkt-line payload size overflow"))?;
    if *total > MAX_PKT_LINE_PAYLOAD_BYTES {
        return Err(err("pkt-line payload exceeds size limit"));
    }
    Ok(())
}

fn add_advertised_ref_count(total: usize) -> io::Result<()> {
    if total > MAX_ADVERTISED_REFS {
        return Err(err("remote advertised too many refs"));
    }
    Ok(())
}

fn response_body_preview(body: &[u8]) -> String {
    let limit = body.len().min(MAX_HTTP_ERROR_BODY_BYTES);
    let mut preview = String::from_utf8_lossy(&body[..limit]).trim().to_string();
    if body.len() > limit {
        preview.push_str("...");
    }
    preview
}

fn normalize_repo_path(path: &str) -> io::Result<String> {
    if path.is_empty() || path.as_bytes().contains(&0) {
        return Err(err("invalid empty or nul-containing repository path"));
    }

    let mut parts = Vec::new();
    for component in Path::new(path).components() {
        match component {
            Component::Normal(part) if part == OsStr::new(".git") => {
                return Err(err("repository path must not enter .git"));
            }
            Component::Normal(part) => {
                let part = part
                    .to_str()
                    .ok_or_else(|| err("repository path must be utf-8"))?;
                if part.is_empty() {
                    return Err(err("repository path contains an empty component"));
                }
                parts.push(part);
            }
            Component::CurDir
            | Component::ParentDir
            | Component::RootDir
            | Component::Prefix(_) => {
                return Err(err("repository path must be relative and normalized"));
            }
        }
    }

    if parts.is_empty() {
        return Err(err("repository path must name a file"));
    }

    Ok(parts.join("/"))
}

fn worktree_path(workdir: &Path, repo_path: &str) -> io::Result<PathBuf> {
    Ok(workdir.join(normalize_repo_path(repo_path)?))
}

fn validate_ref_tail(name: &str) -> io::Result<&str> {
    if name.is_empty()
        || name.as_bytes().contains(&0)
        || name.starts_with('/')
        || name.ends_with('/')
        || name.contains('\\')
    {
        return Err(err("invalid git ref name"));
    }

    for part in name.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Err(err("invalid git ref name"));
        }
    }

    Ok(name)
}

fn validate_branch_name(name: &str) -> io::Result<&str> {
    validate_ref_tail(name)
}

fn validate_refname(refname: &str) -> io::Result<&str> {
    if refname == "HEAD" {
        return Ok(refname);
    }

    for prefix in ["refs/heads/", "refs/remotes/", "refs/tags/"] {
        if let Some(tail) = refname.strip_prefix(prefix) {
            validate_ref_tail(tail)?;
            return Ok(refname);
        }
    }

    Err(err("unsupported git ref name"))
}

fn hash_bytes(obj_type: &str, data: &[u8]) -> [u8; 20] {
    let header = format!("{} {}\0", obj_type, data.len());
    let mut hasher = Sha1::new();
    hasher.update(header.as_bytes());
    hasher.update(data);
    hasher.finalize().into()
}

fn is_zero_oid(hash: &[u8; 20]) -> bool {
    hash.iter().all(|b| *b == 0)
}

fn is_remote_source(source: &str) -> bool {
    source.starts_with("http://") || source.starts_with("https://")
}

fn is_ssh_clone_source(source: &str) -> bool {
    source.starts_with("git@")
        || source.starts_with("ssh://")
        || source.starts_with("git://")
        || source.starts_with("ssh+git://")
}

fn has_http_auth(source: &str) -> bool {
    if !is_remote_source(source) {
        return false;
    }

    let Some(scheme_sep) = source.find("://") else {
        return false;
    };
    let authority = &source[scheme_sep + 3..]
        .split_once('/')
        .map(|(authority, _)| authority)
        .unwrap_or(&source[scheme_sep + 3..]);

    authority.contains('@')
}

fn infer_clone_destination(source: &str) -> io::Result<String> {
    let basename = if is_remote_source(source) || is_ssh_clone_source(source) {
        let trimmed = source.trim_end_matches('/');
        let leaf = if let Some((_, scp_path)) = trimmed.rsplit_once(':') {
            if is_ssh_clone_source(source) && !scp_path.contains('/') {
                scp_path
            } else {
                trimmed
                    .rsplit('/')
                    .next()
                    .filter(|segment| !segment.is_empty())
                    .ok_or_else(|| err("could not determine destination path from source"))?
            }
        } else {
            trimmed
                .rsplit('/')
                .next()
                .filter(|segment| !segment.is_empty())
                .ok_or_else(|| err("could not determine destination path from source"))?
        };
        leaf.to_string()
    } else {
        Path::new(source)
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .filter(|name| !name.is_empty())
            .ok_or_else(|| err("could not determine destination path from source"))?
    };

    Ok(basename
        .strip_suffix(".git")
        .unwrap_or(&basename)
        .to_string())
}

fn prepare_clone_destination(
    dest: &Path,
    source_label: &str,
    default_branch: &str,
) -> io::Result<PathBuf> {
    validate_branch_name(default_branch)?;
    mkdirs(dest)?;

    let dst_git = dest.join(".git");
    mkdirs(&dst_git.join("objects"))?;
    mkdirs(&dst_git.join("refs/heads"))?;
    mkdirs(&dst_git.join("refs/tags"))?;
    mkdirs(&dst_git.join("refs/remotes/origin"))?;

    fs::write(
        dst_git.join("HEAD"),
        format!("ref: refs/heads/{}\n", default_branch),
    )?;
    fs::write(
        dst_git.join("config"),
        format!(
            "[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = false\n\
             [remote \"origin\"]\n\turl = {}\n\tfetch = +refs/heads/*:refs/remotes/origin/*\n",
            source_label
        ),
    )?;

    Ok(dst_git)
}

#[derive(Debug)]
enum PktLine {
    Data(Vec<u8>),
    Flush,
}

fn parse_pkt_lines(data: &[u8]) -> io::Result<Vec<PktLine>> {
    let mut pos = 0usize;
    let mut lines = Vec::new();
    let mut payload_bytes = 0usize;

    while pos + 4 <= data.len() {
        if lines.len() >= MAX_PKT_LINES {
            return Err(err("too many pkt-lines"));
        }
        let len_str =
            std::str::from_utf8(&data[pos..pos + 4]).map_err(|_| err("invalid pkt-line"))?;
        let len = usize::from_str_radix(len_str, 16).map_err(|_| err("invalid pkt-line length"))?;
        pos += 4;

        if len == 0 {
            lines.push(PktLine::Flush);
            continue;
        }
        if len < 4 || pos + len - 4 > data.len() {
            return Err(err("truncated pkt-line"));
        }

        let payload_len = len - 4;
        add_pkt_payload_bytes(&mut payload_bytes, payload_len)?;
        lines.push(PktLine::Data(data[pos..pos + payload_len].to_vec()));
        pos += len - 4;
    }

    if pos != data.len() {
        return Err(err("trailing bytes after pkt-lines"));
    }

    Ok(lines)
}

#[derive(Debug, Default)]
struct RemoteAdvertisement {
    head_hash: Option<[u8; 20]>,
    head_target: Option<String>,
    branches: BTreeMap<String, [u8; 20]>,
    tags: BTreeMap<String, [u8; 20]>,
}

fn parse_advertised_ref(line: &[u8], adv: &mut RemoteAdvertisement) -> io::Result<()> {
    let (ref_part, capability_part) = if let Some(nul) = line.iter().position(|b| *b == 0) {
        (&line[..nul], Some(&line[nul + 1..]))
    } else {
        (line, None)
    };

    let ref_text = std::str::from_utf8(ref_part)
        .map_err(|_| err("invalid advertised ref"))?
        .trim_end_matches('\n');
    if ref_text.is_empty() {
        return Ok(());
    }

    let (hash_hex, refname) = ref_text
        .split_once(' ')
        .ok_or_else(|| err("malformed advertised ref"))?;
    let hash = unhex(hash_hex)?;

    if refname == "HEAD" {
        adv.head_hash = Some(hash);
    } else if refname.starts_with("refs/heads/") {
        validate_refname(refname)?;
        adv.branches.insert(refname.to_string(), hash);
        add_advertised_ref_count(adv.branches.len() + adv.tags.len())?;
    } else if refname.starts_with("refs/tags/") {
        validate_refname(refname)?;
        adv.tags.insert(refname.to_string(), hash);
        add_advertised_ref_count(adv.branches.len() + adv.tags.len())?;
    }

    if let Some(capabilities) = capability_part {
        let caps = std::str::from_utf8(capabilities).map_err(|_| err("invalid capability list"))?;
        for cap in caps.split_whitespace() {
            if let Some(target) = cap.strip_prefix("symref=HEAD:") {
                validate_refname(target)?;
                adv.head_target = Some(target.to_string());
            }
        }
    }

    Ok(())
}

fn fetch_remote_advertisement(url: &str) -> io::Result<RemoteAdvertisement> {
    let info_refs_url = format!(
        "{}/info/refs?service=git-upload-pack",
        url.trim_end_matches('/')
    );
    let req = Request::new(Method::Get, &info_refs_url)
        .map_err(|e| err(&format!("bad info/refs URL: {}", e)))?
        .header("Accept", "application/x-git-upload-pack-advertisement")
        .header("Git-Protocol", "version=0");

    let resp = HttpClient::new()
        .send(&req)
        .map_err(|e| err(&format!("fetch info/refs failed: {}", e)))?;
    if resp.status != 200 {
        let body = response_body_preview(&resp.body);
        return Err(err(&format!(
            "remote advertised refs request failed (HTTP {}): {}",
            resp.status, body
        )));
    }
    if resp.body.len() > MAX_REF_ADVERTISEMENT_BYTES {
        return Err(err("remote advertised refs response exceeds size limit"));
    }

    let lines = parse_pkt_lines(&resp.body)?;
    let mut adv = RemoteAdvertisement::default();
    let mut saw_service = false;
    let mut in_refs = false;

    for line in lines {
        match line {
            PktLine::Flush => {
                if saw_service {
                    in_refs = true;
                }
            }
            PktLine::Data(data) => {
                if !saw_service {
                    let service = std::str::from_utf8(&data)
                        .map_err(|_| err("invalid git service header"))?;
                    if service.trim_end_matches('\n') != "# service=git-upload-pack" {
                        return Err(err("unexpected git service advertisement"));
                    }
                    saw_service = true;
                    continue;
                }
                if !in_refs {
                    continue;
                }
                parse_advertised_ref(&data, &mut adv)?;
            }
        }
    }

    if !saw_service {
        return Err(err("missing git-upload-pack service header"));
    }

    Ok(adv)
}

fn branch_name_from_ref(refname: &str) -> io::Result<String> {
    refname
        .strip_prefix("refs/heads/")
        .map(validate_branch_name)
        .transpose()?
        .map(|name| name.to_string())
        .ok_or_else(|| err("expected refs/heads/* ref"))
}

fn default_branch_ref(adv: &RemoteAdvertisement) -> Option<(String, [u8; 20])> {
    if let Some(target) = adv.head_target.as_ref() {
        if let Some(hash) = adv.branches.get(target) {
            return Some((target.clone(), *hash));
        }
    }

    if let Some(head_hash) = adv.head_hash {
        if let Some((name, hash)) = adv.branches.iter().find(|(_, hash)| **hash == head_hash) {
            return Some((name.clone(), *hash));
        }
    }

    adv.branches
        .iter()
        .next()
        .map(|(name, hash)| (name.clone(), *hash))
}

fn pkt_line_bytes(payload: &[u8]) -> Vec<u8> {
    let mut out = format!("{:04x}", payload.len() + 4).into_bytes();
    out.extend_from_slice(payload);
    out
}

fn fetch_remote_pack(url: &str, wants: &[String]) -> io::Result<Vec<u8>> {
    if wants.is_empty() {
        return Ok(Vec::new());
    }

    let upload_pack_url = format!("{}/git-upload-pack", url.trim_end_matches('/'));
    let mut body = Vec::new();
    for (i, want) in wants.iter().enumerate() {
        let line = if i == 0 {
            format!(
                "want {} side-band-64k ofs-delta no-progress include-tag agent=agentos/1.0\n",
                want
            )
        } else {
            format!("want {}\n", want)
        };
        body.extend_from_slice(&pkt_line_bytes(line.as_bytes()));
    }
    body.extend_from_slice(b"0000");
    body.extend_from_slice(&pkt_line_bytes(b"done\n"));

    let req = Request::new(Method::Post, &upload_pack_url)
        .map_err(|e| err(&format!("bad upload-pack URL: {}", e)))?
        .header("Content-Type", "application/x-git-upload-pack-request")
        .header("Accept", "application/x-git-upload-pack-result")
        .header("Git-Protocol", "version=0")
        .body(body);

    let resp = HttpClient::new()
        .send(&req)
        .map_err(|e| err(&format!("git-upload-pack failed: {}", e)))?;
    if resp.status != 200 {
        let body = response_body_preview(&resp.body);
        return Err(err(&format!(
            "git-upload-pack returned HTTP {}: {}",
            resp.status, body
        )));
    }

    if resp.body.len() > MAX_PACK_BYTES {
        return Err(err("packfile exceeds size limit"));
    }
    if resp.body.starts_with(b"PACK") {
        return Ok(resp.body);
    }

    let mut pack = Vec::new();
    for line in parse_pkt_lines(&resp.body)? {
        match line {
            PktLine::Flush => {}
            PktLine::Data(payload) => {
                if payload == b"NAK\n" || payload.starts_with(b"ACK ") {
                    continue;
                }
                if payload.starts_with(b"ERR ") {
                    let msg = String::from_utf8_lossy(&payload[4..]);
                    return Err(err(&format!("remote upload-pack error: {}", msg.trim())));
                }
                if payload.starts_with(b"PACK") {
                    append_pack_bytes(&mut pack, &payload)?;
                    continue;
                }
                if payload.is_empty() {
                    continue;
                }
                match payload[0] {
                    1 => append_pack_bytes(&mut pack, &payload[1..])?,
                    2 => {}
                    3 => {
                        let msg = String::from_utf8_lossy(&payload[1..]);
                        return Err(err(&format!("remote upload-pack error: {}", msg.trim())));
                    }
                    _ => return Err(err("unexpected upload-pack response payload")),
                }
            }
        }
    }

    if pack.is_empty() {
        return Err(err("git-upload-pack response did not include a packfile"));
    }
    Ok(pack)
}

#[derive(Clone, Debug)]
enum PackedObjectKind {
    Full { obj_type: String, data: Vec<u8> },
    OfsDelta { base_offset: usize, delta: Vec<u8> },
    RefDelta { base_hash: [u8; 20], delta: Vec<u8> },
}

#[derive(Clone, Debug)]
struct PackedObject {
    offset: usize,
    kind: PackedObjectKind,
}

#[derive(Clone, Debug)]
struct ResolvedObject {
    obj_type: String,
    data: Vec<u8>,
    hash: [u8; 20],
}

fn parse_pack_object_header(pack: &[u8], offset: &mut usize) -> io::Result<(u8, usize)> {
    if *offset >= pack.len() {
        return Err(err("truncated pack object header"));
    }

    let mut byte = pack[*offset];
    *offset += 1;

    let obj_type = (byte >> 4) & 0x7;
    let mut size = (byte & 0x0f) as usize;
    let mut shift = 4usize;

    while byte & 0x80 != 0 {
        if *offset >= pack.len() {
            return Err(err("truncated pack object size"));
        }
        byte = pack[*offset];
        *offset += 1;
        if shift >= usize::BITS as usize {
            return Err(err("pack object size is too large"));
        }
        size |= ((byte & 0x7f) as usize)
            .checked_shl(shift as u32)
            .ok_or_else(|| err("pack object size is too large"))?;
        shift += 7;
    }

    Ok((obj_type, size))
}

fn parse_ofs_delta_base(
    pack: &[u8],
    offset: &mut usize,
    object_offset: usize,
) -> io::Result<usize> {
    if *offset >= pack.len() {
        return Err(err("truncated ofs-delta base"));
    }

    let mut byte = pack[*offset];
    *offset += 1;
    let mut distance = (byte & 0x7f) as usize;

    while byte & 0x80 != 0 {
        if *offset >= pack.len() {
            return Err(err("truncated ofs-delta base"));
        }
        byte = pack[*offset];
        *offset += 1;
        distance = distance
            .checked_add(1)
            .and_then(|value| value.checked_shl(7))
            .map(|value| value | ((byte & 0x7f) as usize))
            .ok_or_else(|| err("ofs-delta base distance is too large"))?;
    }

    object_offset
        .checked_sub(distance)
        .ok_or_else(|| err("invalid ofs-delta base distance"))
}

fn inflate_pack_stream(data: &[u8], expected_size: usize) -> io::Result<(Vec<u8>, usize)> {
    if expected_size > MAX_GIT_OBJECT_BYTES {
        return Err(err("pack object exceeds size limit"));
    }
    let cursor = Cursor::new(data);
    let mut decoder = BufZlibDecoder::new(cursor);
    let out = read_to_end_limited(&mut decoder, expected_size, "pack object")?;
    if out.len() != expected_size {
        return Err(err("pack object size mismatch"));
    }
    let consumed = decoder.get_ref().position() as usize;
    Ok((out, consumed))
}

fn parse_packfile(pack: &[u8]) -> io::Result<Vec<PackedObject>> {
    if pack.len() < 12 + 20 {
        return Err(err("packfile too small"));
    }
    if &pack[..4] != b"PACK" {
        return Err(err("invalid packfile signature"));
    }

    let version = u32::from_be_bytes(pack[4..8].try_into().unwrap());
    if version != 2 && version != 3 {
        return Err(err(&format!("unsupported packfile version {}", version)));
    }
    let object_count = u32::from_be_bytes(pack[8..12].try_into().unwrap()) as usize;
    let pack_end = pack.len() - 20;
    let mut offset = 12usize;
    let max_count_by_bytes = pack_end.saturating_sub(offset);
    if object_count > MAX_PACK_OBJECTS || object_count > max_count_by_bytes {
        return Err(err("packfile object count is too large"));
    }
    let mut objects = Vec::new();
    try_reserve_exact(&mut objects, object_count, "pack object table")?;
    let mut inflated_bytes = 0usize;

    for _ in 0..object_count {
        if offset >= pack_end {
            return Err(err("truncated packfile"));
        }

        let object_offset = offset;
        let (obj_type, object_size) = parse_pack_object_header(pack, &mut offset)?;

        let kind = match obj_type {
            1 | 2 | 3 | 4 => {
                let obj_type = match obj_type {
                    1 => "commit",
                    2 => "tree",
                    3 => "blob",
                    4 => "tag",
                    _ => unreachable!(),
                };
                let (data, consumed) = inflate_pack_stream(&pack[offset..pack_end], object_size)?;
                offset += consumed;
                add_pack_inflated_bytes(&mut inflated_bytes, data.len())?;
                PackedObjectKind::Full {
                    obj_type: obj_type.to_string(),
                    data,
                }
            }
            6 => {
                let base_offset = parse_ofs_delta_base(pack, &mut offset, object_offset)?;
                let (delta, consumed) = inflate_pack_stream(&pack[offset..pack_end], object_size)?;
                offset += consumed;
                add_pack_inflated_bytes(&mut inflated_bytes, delta.len())?;
                PackedObjectKind::OfsDelta { base_offset, delta }
            }
            7 => {
                if offset + 20 > pack_end {
                    return Err(err("truncated ref-delta base"));
                }
                let mut base_hash = [0u8; 20];
                base_hash.copy_from_slice(&pack[offset..offset + 20]);
                offset += 20;
                let (delta, consumed) = inflate_pack_stream(&pack[offset..pack_end], object_size)?;
                offset += consumed;
                add_pack_inflated_bytes(&mut inflated_bytes, delta.len())?;
                PackedObjectKind::RefDelta { base_hash, delta }
            }
            _ => return Err(err(&format!("unsupported pack object type {}", obj_type))),
        };

        objects.push(PackedObject {
            offset: object_offset,
            kind,
        });
    }

    Ok(objects)
}

fn read_delta_varint(data: &[u8], pos: &mut usize) -> io::Result<usize> {
    let mut value = 0usize;
    let mut shift = 0usize;

    loop {
        if *pos >= data.len() {
            return Err(err("truncated delta header"));
        }
        let byte = data[*pos];
        *pos += 1;

        if shift >= usize::BITS as usize {
            return Err(err("delta varint is too large"));
        }
        value |= ((byte & 0x7f) as usize)
            .checked_shl(shift as u32)
            .ok_or_else(|| err("delta varint is too large"))?;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
        shift += 7;
    }
}

fn ensure_delta_output_room(
    current_len: usize,
    additional_len: usize,
    result_size: usize,
) -> io::Result<()> {
    let next_len = current_len
        .checked_add(additional_len)
        .ok_or_else(|| err("delta result size overflow"))?;
    if next_len > result_size {
        return Err(err("delta result exceeds declared size"));
    }
    Ok(())
}

fn apply_delta(base: &[u8], delta: &[u8]) -> io::Result<Vec<u8>> {
    let mut pos = 0usize;
    let base_size = read_delta_varint(delta, &mut pos)?;
    if base_size != base.len() {
        return Err(err("delta base size mismatch"));
    }
    let result_size = read_delta_varint(delta, &mut pos)?;
    if result_size > MAX_GIT_OBJECT_BYTES {
        return Err(err("delta result exceeds size limit"));
    }
    let mut out = Vec::new();
    try_reserve_exact(&mut out, result_size, "delta result")?;

    while pos < delta.len() {
        let opcode = delta[pos];
        pos += 1;

        if opcode & 0x80 != 0 {
            let mut copy_offset = 0usize;
            let mut copy_size = 0usize;

            if opcode & 0x01 != 0 {
                copy_offset |= delta
                    .get(pos)
                    .copied()
                    .ok_or_else(|| err("truncated delta copy"))?
                    as usize;
                pos += 1;
            }
            if opcode & 0x02 != 0 {
                copy_offset |= (delta
                    .get(pos)
                    .copied()
                    .ok_or_else(|| err("truncated delta copy"))?
                    as usize)
                    << 8;
                pos += 1;
            }
            if opcode & 0x04 != 0 {
                copy_offset |= (delta
                    .get(pos)
                    .copied()
                    .ok_or_else(|| err("truncated delta copy"))?
                    as usize)
                    << 16;
                pos += 1;
            }
            if opcode & 0x08 != 0 {
                copy_offset |= (delta
                    .get(pos)
                    .copied()
                    .ok_or_else(|| err("truncated delta copy"))?
                    as usize)
                    << 24;
                pos += 1;
            }

            if opcode & 0x10 != 0 {
                copy_size |= delta
                    .get(pos)
                    .copied()
                    .ok_or_else(|| err("truncated delta copy"))?
                    as usize;
                pos += 1;
            }
            if opcode & 0x20 != 0 {
                copy_size |= (delta
                    .get(pos)
                    .copied()
                    .ok_or_else(|| err("truncated delta copy"))?
                    as usize)
                    << 8;
                pos += 1;
            }
            if opcode & 0x40 != 0 {
                copy_size |= (delta
                    .get(pos)
                    .copied()
                    .ok_or_else(|| err("truncated delta copy"))?
                    as usize)
                    << 16;
                pos += 1;
            }
            if copy_size == 0 {
                copy_size = 0x10000;
            }

            let end = copy_offset
                .checked_add(copy_size)
                .ok_or_else(|| err("delta copy overflow"))?;
            if end > base.len() {
                return Err(err("delta copy exceeds base object"));
            }
            ensure_delta_output_room(out.len(), copy_size, result_size)?;
            out.extend_from_slice(&base[copy_offset..end]);
        } else if opcode != 0 {
            let insert_len = opcode as usize;
            let end = pos
                .checked_add(insert_len)
                .ok_or_else(|| err("delta insert overflow"))?;
            if end > delta.len() {
                return Err(err("truncated delta insert"));
            }
            ensure_delta_output_room(out.len(), insert_len, result_size)?;
            out.extend_from_slice(&delta[pos..end]);
            pos = end;
        } else {
            return Err(err("invalid delta opcode"));
        }
    }

    if out.len() != result_size {
        return Err(err("delta result size mismatch"));
    }

    Ok(out)
}

fn maybe_read_local_object(git_dir: &Path, hash: &[u8; 20]) -> io::Result<Option<ResolvedObject>> {
    let h = hex(hash);
    let path = git_dir.join("objects").join(&h[..2]).join(&h[2..]);
    if !path.exists() {
        return Ok(None);
    }
    let (obj_type, data) = read_object(git_dir, hash)?;
    Ok(Some(ResolvedObject {
        obj_type,
        data,
        hash: *hash,
    }))
}

fn find_entry_by_hash(
    target: &[u8; 20],
    git_dir: &Path,
    objects: &[PackedObject],
    offset_to_index: &HashMap<usize, usize>,
    memo: &mut [Option<ResolvedObject>],
    visiting: &mut [bool],
    resolved_bytes: &mut usize,
) -> io::Result<Option<usize>> {
    for idx in 0..objects.len() {
        if visiting[idx] {
            continue;
        }
        let resolved = resolve_packed_object(
            idx,
            git_dir,
            objects,
            offset_to_index,
            memo,
            visiting,
            resolved_bytes,
        )?;
        if resolved.hash == *target {
            return Ok(Some(idx));
        }
    }

    Ok(None)
}

fn resolve_packed_object(
    idx: usize,
    git_dir: &Path,
    objects: &[PackedObject],
    offset_to_index: &HashMap<usize, usize>,
    memo: &mut [Option<ResolvedObject>],
    visiting: &mut [bool],
    resolved_bytes: &mut usize,
) -> io::Result<ResolvedObject> {
    if let Some(resolved) = memo[idx].as_ref() {
        return Ok(resolved.clone());
    }
    if visiting[idx] {
        return Err(err("cyclic pack delta dependency"));
    }
    visiting[idx] = true;

    let resolved = match &objects[idx].kind {
        PackedObjectKind::Full { obj_type, data } => ResolvedObject {
            obj_type: obj_type.clone(),
            data: data.clone(),
            hash: hash_bytes(obj_type, data),
        },
        PackedObjectKind::OfsDelta { base_offset, delta } => {
            let base_idx = *offset_to_index
                .get(base_offset)
                .ok_or_else(|| err("missing ofs-delta base object"))?;
            let base = resolve_packed_object(
                base_idx,
                git_dir,
                objects,
                offset_to_index,
                memo,
                visiting,
                resolved_bytes,
            )?;
            let data = apply_delta(&base.data, delta)?;
            let hash = hash_bytes(&base.obj_type, &data);
            ResolvedObject {
                obj_type: base.obj_type,
                data,
                hash,
            }
        }
        PackedObjectKind::RefDelta { base_hash, delta } => {
            let base = if let Some(local) = maybe_read_local_object(git_dir, base_hash)? {
                local
            } else if let Some(base_idx) = find_entry_by_hash(
                base_hash,
                git_dir,
                objects,
                offset_to_index,
                memo,
                visiting,
                resolved_bytes,
            )? {
                resolve_packed_object(
                    base_idx,
                    git_dir,
                    objects,
                    offset_to_index,
                    memo,
                    visiting,
                    resolved_bytes,
                )?
            } else {
                return Err(err("missing ref-delta base object"));
            };

            let data = apply_delta(&base.data, delta)?;
            let hash = hash_bytes(&base.obj_type, &data);
            ResolvedObject {
                obj_type: base.obj_type,
                data,
                hash,
            }
        }
    };

    visiting[idx] = false;
    add_pack_resolved_bytes(resolved_bytes, resolved.data.len())?;
    memo[idx] = Some(resolved.clone());
    Ok(resolved)
}

fn store_pack_objects(git_dir: &Path, pack: &[u8]) -> io::Result<()> {
    if pack.is_empty() {
        return Ok(());
    }

    let objects = parse_packfile(pack)?;
    let offset_to_index: HashMap<usize, usize> = objects
        .iter()
        .enumerate()
        .map(|(idx, obj)| (obj.offset, idx))
        .collect();
    let mut memo: Vec<Option<ResolvedObject>> = vec![None; objects.len()];
    let mut visiting = vec![false; objects.len()];
    let mut resolved_bytes = 0usize;

    for idx in 0..objects.len() {
        let resolved = resolve_packed_object(
            idx,
            git_dir,
            &objects,
            &offset_to_index,
            &mut memo,
            &mut visiting,
            &mut resolved_bytes,
        )?;
        let stored = hash_object(git_dir, &resolved.obj_type, &resolved.data)?;
        if stored != resolved.hash {
            return Err(err("pack object hash mismatch"));
        }
    }
    Ok(())
}

fn cmd_clone_remote(source: &str, dest: &Path) -> io::Result<()> {
    if dest.exists() && !dir_is_empty(dest)? {
        return Err(err(&format!(
            "destination path '{}' already exists and is not an empty directory",
            dest.display()
        )));
    }

    let advertisement = fetch_remote_advertisement(source)?;
    let mut wants: Vec<String> = Vec::new();
    let mut seen = HashSet::new();
    for hash in advertisement.branches.values() {
        if !is_zero_oid(hash) {
            let hash_hex = hex(hash);
            if seen.insert(hash_hex.clone()) {
                wants.push(hash_hex);
            }
        }
    }
    if wants.is_empty() {
        if let Some(head_hash) = advertisement.head_hash {
            if !is_zero_oid(&head_hash) {
                wants.push(hex(&head_hash));
            }
        }
    }

    let default_ref = if let Some((refname, hash)) = default_branch_ref(&advertisement) {
        Some((refname.clone(), branch_name_from_ref(&refname)?, hash))
    } else {
        None
    };
    let default_branch = default_ref
        .as_ref()
        .map(|(_, branch, _)| branch.clone())
        .or_else(|| {
            advertisement
                .head_target
                .as_ref()
                .and_then(|target| branch_name_from_ref(target).ok())
        })
        .unwrap_or_else(|| "main".to_string());

    let pack = fetch_remote_pack(source, &wants)?;

    let dst_git = prepare_clone_destination(dest, source, &default_branch)?;
    store_pack_objects(&dst_git, &pack)?;

    for (refname, hash) in &advertisement.branches {
        if is_zero_oid(hash) {
            continue;
        }
        let branch = branch_name_from_ref(refname)?;
        update_ref(&dst_git, &format!("refs/remotes/origin/{}", branch), hash)?;
    }

    for (refname, hash) in &advertisement.tags {
        if is_zero_oid(hash) {
            continue;
        }
        update_ref(&dst_git, refname, hash)?;
    }

    let default_hash = default_ref
        .as_ref()
        .map(|(_, _, hash)| *hash)
        .or(advertisement.head_hash)
        .filter(|hash| !is_zero_oid(hash));

    if let Some(hash) = default_hash {
        update_ref(&dst_git, &format!("refs/heads/{}", default_branch), &hash)?;
        cmd_checkout(dest, &default_branch, false)?;
    }

    Ok(())
}

// ─── Object store ───────────────────────────────────────────────────────────

fn hash_object(git_dir: &Path, obj_type: &str, data: &[u8]) -> io::Result<[u8; 20]> {
    if data.len() > MAX_GIT_OBJECT_BYTES {
        return Err(err("git object exceeds size limit"));
    }
    let header = format!("{} {}\0", obj_type, data.len());
    let mut hasher = Sha1::new();
    hasher.update(header.as_bytes());
    hasher.update(data);
    let hash: [u8; 20] = hasher.finalize().into();

    let h = hex(&hash);
    let dir = git_dir.join("objects").join(&h[..2]);
    let path = dir.join(&h[2..]);
    if !path.exists() {
        mkdirs(&dir)?;
        let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
        enc.write_all(header.as_bytes())?;
        enc.write_all(data)?;
        fs::write(&path, enc.finish()?)?;
    }
    Ok(hash)
}

fn read_object(git_dir: &Path, hash: &[u8; 20]) -> io::Result<(String, Vec<u8>)> {
    let h = hex(hash);
    let path = git_dir.join("objects").join(&h[..2]).join(&h[2..]);
    let read_limit = MAX_GIT_OBJECT_BYTES
        .checked_add(MAX_GIT_OBJECT_HEADER_BYTES)
        .ok_or_else(|| err("git object size limit is too large"))?;
    let compressed = read_file_limited(&path, read_limit, "git object")?;
    let mut dec = ZlibDecoder::new(&compressed[..]);
    let buf = read_to_end_limited(&mut dec, read_limit, "git object")?;

    let nul = buf
        .iter()
        .position(|&b| b == 0)
        .ok_or_else(|| err("no nul in object"))?;
    let header =
        std::str::from_utf8(&buf[..nul]).map_err(|_| err("invalid object header encoding"))?;
    let (typ, size) = header
        .split_once(' ')
        .ok_or_else(|| err("malformed object header"))?;
    let size: usize = size.parse().map_err(|_| err("invalid object size"))?;
    if size > MAX_GIT_OBJECT_BYTES {
        return Err(err("git object exceeds size limit"));
    }
    if buf.len() - nul - 1 != size {
        return Err(err("git object size mismatch"));
    }
    Ok((typ.to_string(), buf[nul + 1..].to_vec()))
}

// ─── Index ──────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct IndexEntry {
    mode: u32,
    sha1: [u8; 20],
    name: String,
}

fn read_index(git_dir: &Path) -> io::Result<Vec<IndexEntry>> {
    let path = git_dir.join("index");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let data = read_file_limited(&path, MAX_INDEX_BYTES, "git index")?;
    if data.len() < 12 || &data[0..4] != b"DIRC" {
        return Err(err("invalid index file"));
    }
    let version = u32::from_be_bytes(data[4..8].try_into().unwrap());
    if version != 2 {
        return Err(err(&format!("unsupported index version {}", version)));
    }
    let count = u32::from_be_bytes(data[8..12].try_into().unwrap()) as usize;
    let max_count_by_bytes = data.len().saturating_sub(12) / 62;
    if count > MAX_INDEX_ENTRIES || count > max_count_by_bytes {
        return Err(err("index entry count is too large"));
    }

    let mut entries = Vec::new();
    try_reserve_exact(&mut entries, count, "index entry table")?;
    let mut pos = 12;

    for _ in 0..count {
        if pos + 62 > data.len() {
            return Err(err("truncated index"));
        }
        // Stat fields at pos+0..pos+24 (ctime, mtime, dev, ino) - skip
        let mode = u32::from_be_bytes(data[pos + 24..pos + 28].try_into().unwrap());
        // Skip uid(4), gid(4), size(4) at pos+28..pos+40
        let mut sha1 = [0u8; 20];
        sha1.copy_from_slice(&data[pos + 40..pos + 60]);
        // Flags at pos+60..pos+62
        // Find name (NUL-terminated after fixed 62 bytes)
        let name_start = pos + 62;
        let nul_offset = data[name_start..]
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| err("unterminated index entry name"))?;
        let name = String::from_utf8(data[name_start..name_start + nul_offset].to_vec())
            .map_err(|_| err("invalid entry name"))?;
        let name = normalize_repo_path(&name)?;

        entries.push(IndexEntry { mode, sha1, name });

        // Advance past padding: entry padded to 8-byte boundary
        let entry_len = 62 + nul_offset;
        pos += (entry_len + 8) & !7;
    }

    Ok(entries)
}

fn write_index(git_dir: &Path, entries: &[IndexEntry]) -> io::Result<()> {
    let entry_count = u32::try_from(entries.len()).map_err(|_| err("too many index entries"))?;
    let mut buf = Vec::new();
    buf.extend_from_slice(b"DIRC");
    buf.extend_from_slice(&2u32.to_be_bytes());
    buf.extend_from_slice(&entry_count.to_be_bytes());

    for entry in entries {
        let name = normalize_repo_path(&entry.name)?;
        if name.len() > 0xFFF {
            return Err(err("index entry name is too long"));
        }
        let entry_start = buf.len();
        // ctime(8) + mtime(8) + dev(4) + ino(4) = 24 bytes of zeros
        buf.extend_from_slice(&[0u8; 24]);
        buf.extend_from_slice(&entry.mode.to_be_bytes());
        // uid(4) + gid(4) + size(4) = 12 bytes of zeros
        buf.extend_from_slice(&[0u8; 12]);
        buf.extend_from_slice(&entry.sha1);
        // Flags: name length in lower 12 bits
        buf.extend_from_slice(&(name.len() as u16).to_be_bytes());
        buf.extend_from_slice(name.as_bytes());
        // Pad to 8-byte boundary (1-8 NUL bytes)
        let entry_len = buf.len() - entry_start;
        let padded = (entry_len + 8) & !7;
        buf.resize(entry_start + padded, 0);
    }

    // SHA-1 checksum of entire index
    let checksum: [u8; 20] = Sha1::digest(&buf).into();
    buf.extend_from_slice(&checksum);

    fs::write(git_dir.join("index"), &buf)
}

// ─── Refs ───────────────────────────────────────────────────────────────────

fn resolve_ref(git_dir: &Path, refname: &str) -> io::Result<Option<[u8; 20]>> {
    let refname = validate_refname(refname)?;
    let ref_path = git_dir.join(refname);
    if !ref_path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&ref_path)?;
    let content = content.trim();
    if let Some(target) = content.strip_prefix("ref: ") {
        resolve_ref(git_dir, target)
    } else {
        Ok(Some(unhex(content)?))
    }
}

fn resolve_head(git_dir: &Path) -> io::Result<Option<[u8; 20]>> {
    resolve_ref(git_dir, "HEAD")
}

fn head_branch(git_dir: &Path) -> io::Result<Option<String>> {
    let head = fs::read_to_string(git_dir.join("HEAD"))?;
    let head = head.trim();
    Ok(head.strip_prefix("ref: refs/heads/").map(|s| s.to_string()))
}

fn update_ref(git_dir: &Path, refname: &str, hash: &[u8; 20]) -> io::Result<()> {
    let refname = validate_refname(refname)?;
    let ref_path = git_dir.join(refname);
    if let Some(parent) = ref_path.parent() {
        mkdirs(parent)?;
    }
    fs::write(&ref_path, format!("{}\n", hex(hash)))
}

// ─── Tree operations ────────────────────────────────────────────────────────

fn build_tree(git_dir: &Path, entries: &[IndexEntry]) -> io::Result<[u8; 20]> {
    build_tree_at(git_dir, entries, "")
}

fn build_tree_at(git_dir: &Path, entries: &[IndexEntry], prefix: &str) -> io::Result<[u8; 20]> {
    struct TreeEntry {
        mode: String,
        name: String,
        hash: [u8; 20],
    }

    let mut tree_entries: Vec<TreeEntry> = Vec::new();
    let mut subdirs: BTreeMap<String, Vec<IndexEntry>> = BTreeMap::new();

    for entry in entries {
        let relative = if prefix.is_empty() {
            &entry.name[..]
        } else if let Some(rest) = entry.name.strip_prefix(prefix) {
            rest
        } else {
            continue;
        };

        if let Some(slash) = relative.find('/') {
            let dir = &relative[..slash];
            subdirs
                .entry(dir.to_string())
                .or_default()
                .push(entry.clone());
        } else {
            tree_entries.push(TreeEntry {
                mode: format!("{:o}", entry.mode),
                name: relative.to_string(),
                hash: entry.sha1,
            });
        }
    }

    for (dir, sub_entries) in &subdirs {
        let sub_prefix = if prefix.is_empty() {
            format!("{}/", dir)
        } else {
            format!("{}{}/", prefix, dir)
        };
        let subtree_hash = build_tree_at(git_dir, sub_entries, &sub_prefix)?;
        tree_entries.push(TreeEntry {
            mode: "40000".to_string(),
            name: dir.clone(),
            hash: subtree_hash,
        });
    }

    // Sort: directories get trailing / for comparison
    tree_entries.sort_by(|a, b| {
        let ak = if a.mode == "40000" {
            format!("{}/", a.name)
        } else {
            a.name.clone()
        };
        let bk = if b.mode == "40000" {
            format!("{}/", b.name)
        } else {
            b.name.clone()
        };
        ak.cmp(&bk)
    });

    let mut data = Vec::new();
    for te in &tree_entries {
        data.extend_from_slice(te.mode.as_bytes());
        data.push(b' ');
        data.extend_from_slice(te.name.as_bytes());
        data.push(0);
        data.extend_from_slice(&te.hash);
    }

    hash_object(git_dir, "tree", &data)
}

/// Read a tree recursively into flat (path, mode, hash) entries.
fn read_tree_entries(git_dir: &Path, hash: &[u8; 20], prefix: &str) -> io::Result<Vec<IndexEntry>> {
    let (typ, data) = read_object(git_dir, hash)?;
    if typ != "tree" {
        return Err(err("expected tree object"));
    }

    let mut entries = Vec::new();
    let mut pos = 0;

    while pos < data.len() {
        let space = data[pos..]
            .iter()
            .position(|&b| b == b' ')
            .ok_or_else(|| err("bad tree entry"))?;
        let mode_str =
            std::str::from_utf8(&data[pos..pos + space]).map_err(|_| err("bad tree mode"))?;
        let mode = u32::from_str_radix(mode_str, 8).map_err(|_| err("bad tree mode number"))?;
        pos += space + 1;

        let nul = data[pos..]
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| err("bad tree entry name"))?;
        let name = std::str::from_utf8(&data[pos..pos + nul]).map_err(|_| err("bad name"))?;
        if name.contains('/') {
            return Err(err("tree entry name must not contain '/'"));
        }
        if normalize_repo_path(name)? != name {
            return Err(err("tree entry name must be normalized"));
        }
        pos += nul + 1;

        if pos + 20 > data.len() {
            return Err(err("truncated tree hash"));
        }
        let mut hash_buf = [0u8; 20];
        hash_buf.copy_from_slice(&data[pos..pos + 20]);
        pos += 20;

        let full_name = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{}{}", prefix, name)
        };

        if mode == 0o40000 {
            let sub = read_tree_entries(git_dir, &hash_buf, &format!("{}/", full_name))?;
            entries.extend(sub);
        } else {
            let full_name = normalize_repo_path(&full_name)?;
            entries.push(IndexEntry {
                mode,
                sha1: hash_buf,
                name: full_name,
            });
        }
    }

    Ok(entries)
}

/// Extract the tree hash from a commit object.
fn commit_tree(git_dir: &Path, commit_hash: &[u8; 20]) -> io::Result<[u8; 20]> {
    let (typ, data) = read_object(git_dir, commit_hash)?;
    if typ != "commit" {
        return Err(err("not a commit object"));
    }
    let text = String::from_utf8_lossy(&data);
    let tree_hex = text
        .lines()
        .find_map(|l| l.strip_prefix("tree "))
        .ok_or_else(|| err("no tree line in commit"))?;
    unhex(tree_hex)
}

// ─── Commands ───────────────────────────────────────────────────────────────

fn cmd_init(path: &Path, quiet: bool) -> io::Result<()> {
    let git_dir = path.join(".git");
    // Create directories one at a time to avoid create_dir_all issues on WASI
    for dir in &[
        path.to_path_buf(),
        git_dir.clone(),
        git_dir.join("objects"),
        git_dir.join("refs"),
        git_dir.join("refs/heads"),
        git_dir.join("refs/tags"),
    ] {
        match fs::create_dir(dir) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(err(&format!("mkdir {}: {}", dir.display(), e))),
        }
    }
    fs::write(git_dir.join("HEAD"), "ref: refs/heads/main\n")
        .map_err(|e| err(&format!("write HEAD: {}", e)))?;
    fs::write(
        git_dir.join("config"),
        "[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = false\n",
    )
    .map_err(|e| err(&format!("write config: {}", e)))?;
    if !quiet {
        print_stdout_line(format_args!(
            "Initialized empty Git repository in {}/.git/",
            path.display()
        ))?;
    }
    Ok(())
}

fn collect_add_paths(workdir: &Path, rel_path: &str, out: &mut Vec<String>) -> io::Result<()> {
    if rel_path == ".git" || rel_path.starts_with(".git/") || rel_path.ends_with("/.git") {
        return Ok(());
    }

    if rel_path == "." {
        for entry in fs::read_dir(workdir)? {
            let entry = entry?;
            let name = entry
                .file_name()
                .to_str()
                .ok_or_else(|| err("repository path must be utf-8"))?
                .to_owned();
            if name == ".git" {
                continue;
            }
            collect_add_paths(workdir, &name, out)?;
        }
        return Ok(());
    }

    let repo_path = normalize_repo_path(rel_path)?;
    let file_path = workdir.join(&repo_path);
    if file_path.is_dir() {
        for entry in fs::read_dir(&file_path)? {
            let entry = entry?;
            let name = entry
                .file_name()
                .to_str()
                .ok_or_else(|| err("repository path must be utf-8"))?
                .to_owned();
            if name == ".git" {
                continue;
            }
            collect_add_paths(workdir, &format!("{repo_path}/{name}"), out)?;
        }
        return Ok(());
    }

    if file_path.exists() {
        out.push(repo_path);
    }
    Ok(())
}

fn cmd_add(workdir: &Path, paths: &[String]) -> io::Result<()> {
    let git_dir = workdir.join(".git");
    let mut entries = read_index(&git_dir).map_err(|e| {
        err(&format!(
            "cannot read index at {}: {}",
            git_dir.display(),
            e
        ))
    })?;

    for rel_path in paths {
        let mut repo_paths = Vec::new();
        collect_add_paths(workdir, rel_path, &mut repo_paths)?;
        if repo_paths.is_empty() {
            return Err(err(&format!(
                "pathspec '{}' did not match any files",
                rel_path
            )));
        }
        for repo_path in repo_paths {
            let file_path = workdir.join(&repo_path);
            if !file_path.exists() {
                return Err(err(&format!(
                    "pathspec '{}' did not match any files (looked at {})",
                    rel_path,
                    file_path.display()
                )));
            }
            let content = read_file_limited(&file_path, MAX_GIT_OBJECT_BYTES, "file")?;
            let hash = hash_object(&git_dir, "blob", &content)?;

            entries.retain(|e| e.name != repo_path);
            entries.push(IndexEntry {
                mode: 0o100644,
                sha1: hash,
                name: repo_path,
            });
        }
    }

    entries.sort_by(|a, b| a.name.cmp(&b.name));
    write_index(&git_dir, &entries)
}

fn cmd_commit(workdir: &Path, message: &str) -> io::Result<()> {
    let git_dir = workdir.join(".git");
    let entries = read_index(&git_dir)?;
    if entries.is_empty() {
        return Err(err("nothing to commit"));
    }

    let tree_hash = build_tree(&git_dir, &entries)?;
    let parent = resolve_head(&git_dir)?;

    let name = std::env::var("GIT_AUTHOR_NAME")
        .or_else(|_| std::env::var("GIT_COMMITTER_NAME"))
        .unwrap_or_else(|_| "secure-exec".to_string());
    let email = std::env::var("GIT_AUTHOR_EMAIL")
        .or_else(|_| std::env::var("GIT_COMMITTER_EMAIL"))
        .unwrap_or_else(|_| "agent@os".to_string());
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(1700000000);

    let mut commit = String::new();
    commit.push_str(&format!("tree {}\n", hex(&tree_hash)));
    if let Some(p) = parent {
        commit.push_str(&format!("parent {}\n", hex(&p)));
    }
    commit.push_str(&format!(
        "author {} <{}> {} +0000\n",
        name, email, timestamp
    ));
    commit.push_str(&format!(
        "committer {} <{}> {} +0000\n",
        name, email, timestamp
    ));
    commit.push_str(&format!("\n{}\n", message));

    let commit_hash = hash_object(&git_dir, "commit", commit.as_bytes())?;

    // Update the ref that HEAD points to
    let head_content = fs::read_to_string(git_dir.join("HEAD"))?;
    let head_content = head_content.trim();
    if let Some(refname) = head_content.strip_prefix("ref: ") {
        update_ref(&git_dir, refname, &commit_hash)?;
    } else {
        fs::write(git_dir.join("HEAD"), format!("{}\n", hex(&commit_hash)))?;
    }

    Ok(())
}

fn cmd_rev_parse(workdir: &Path, args: &[String]) -> io::Result<()> {
    let git_dir = workdir.join(".git");
    if args.len() != 1 {
        return Err(err("usage: git rev-parse <ref>"));
    }

    let hash = resolve_ref(&git_dir, &args[0])?.ok_or_else(|| {
        err(&format!(
            "unknown revision or path not in the working tree: {}",
            args[0]
        ))
    })?;
    print_stdout_line(format_args!("{}", hex(&hash)))
}

fn cmd_branch(workdir: &Path) -> io::Result<()> {
    let git_dir = workdir.join(".git");
    let heads_dir = git_dir.join("refs/heads");
    let current = head_branch(&git_dir)?;

    let mut branches: Vec<String> = Vec::new();
    if heads_dir.exists() {
        for entry in fs::read_dir(&heads_dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            if name == "." || name == ".." {
                continue;
            }
            if entry.file_type()?.is_file() {
                branches.push(name);
            }
        }
    }
    branches.sort();

    for branch in &branches {
        if Some(branch.as_str()) == current.as_deref() {
            print_stdout_line(format_args!("* {}", branch))?;
        } else {
            print_stdout_line(format_args!("  {}", branch))?;
        }
    }

    Ok(())
}

fn cmd_checkout(workdir: &Path, target: &str, create_branch: bool) -> io::Result<()> {
    let git_dir = workdir.join(".git");

    if create_branch {
        validate_branch_name(target)?;
        let head = resolve_head(&git_dir)?.ok_or_else(|| err("HEAD not found for new branch"))?;
        update_ref(&git_dir, &format!("refs/heads/{}", target), &head)?;
        fs::write(
            git_dir.join("HEAD"),
            format!("ref: refs/heads/{}\n", target),
        )?;
        return Ok(());
    }

    // Resolve target: local branch first, then DWIM remote tracking
    validate_branch_name(target)?;
    let branch_ref = format!("refs/heads/{}", target);
    let commit_hash = if let Some(h) = resolve_ref(&git_dir, &branch_ref)? {
        fs::write(git_dir.join("HEAD"), format!("ref: {}\n", branch_ref))?;
        h
    } else {
        let remote_ref = format!("refs/remotes/origin/{}", target);
        if let Some(h) = resolve_ref(&git_dir, &remote_ref)? {
            update_ref(&git_dir, &branch_ref, &h)?;
            fs::write(git_dir.join("HEAD"), format!("ref: {}\n", branch_ref))?;
            h
        } else {
            return Err(err(&format!(
                "pathspec '{}' did not match any branch",
                target
            )));
        }
    };

    // Read target tree
    let tree_hash = commit_tree(&git_dir, &commit_hash)?;
    let new_entries = read_tree_entries(&git_dir, &tree_hash, "")?;

    // Clean up files from current index that aren't in target
    let old_entries = read_index(&git_dir)?;
    let new_names: HashSet<&str> = new_entries.iter().map(|e| e.name.as_str()).collect();
    for old in &old_entries {
        if !new_names.contains(old.name.as_str()) {
            let p = worktree_path(workdir, &old.name)?;
            if p.exists() {
                fs::remove_file(&p).map_err(|e| err(&format!("remove {}: {}", p.display(), e)))?;
            }
        }
    }

    // Write files from target tree
    for entry in &new_entries {
        let p = worktree_path(workdir, &entry.name)?;
        if let Some(parent) = p.parent() {
            mkdirs(parent)?;
        }
        let (_, blob) = read_object(&git_dir, &entry.sha1)?;
        fs::write(&p, &blob)?;
    }

    // Update index
    write_index(&git_dir, &new_entries)?;

    Ok(())
}

fn cmd_clone_local(source: &Path, dest: &Path) -> io::Result<()> {
    let src_git = source.join(".git");
    if !src_git.is_dir() {
        return Err(err(&format!(
            "'{}' does not appear to be a git repository",
            source.display()
        )));
    }

    if dest.exists() && !dir_is_empty(dest)? {
        return Err(err(&format!(
            "destination path '{}' already exists and is not an empty directory",
            dest.display()
        )));
    }

    mkdirs(dest)?;

    let dst_git = dest.join(".git");

    // Init destination
    mkdirs(&dst_git.join("objects"))?;
    mkdirs(&dst_git.join("refs/heads"))?;
    mkdirs(&dst_git.join("refs/tags"))?;
    mkdirs(&dst_git.join("refs/remotes/origin"))?;

    // Copy all objects
    copy_dir_recursive(&src_git.join("objects"), &dst_git.join("objects"))?;

    // Create remote tracking branches from source heads
    let src_heads = src_git.join("refs/heads");
    if src_heads.exists() {
        copy_dir_recursive(&src_heads, &dst_git.join("refs/remotes/origin"))?;
    }

    // Determine default branch from source HEAD
    let src_head = fs::read_to_string(src_git.join("HEAD"))?;
    let src_head = src_head.trim();
    let default_branch = src_head
        .strip_prefix("ref: refs/heads/")
        .unwrap_or("main")
        .to_string();
    validate_branch_name(&default_branch)?;

    // Create local branch for default
    let remote_ref = format!("refs/remotes/origin/{}", default_branch);
    let mut has_default_branch = false;
    if let Some(hash) = resolve_ref(&dst_git, &remote_ref)? {
        update_ref(&dst_git, &format!("refs/heads/{}", default_branch), &hash)?;
        has_default_branch = true;
    }

    // Set HEAD and write config
    fs::write(
        dst_git.join("HEAD"),
        format!("ref: refs/heads/{}\n", default_branch),
    )?;
    fs::write(
        dst_git.join("config"),
        format!(
            "[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = false\n\
             [remote \"origin\"]\n\turl = {}\n\tfetch = +refs/heads/*:refs/remotes/origin/*\n",
            source.display()
        ),
    )?;

    // Checkout working tree
    if has_default_branch {
        cmd_checkout(dest, &default_branch, false)?;
    }

    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> io::Result<()> {
    mkdirs(dst)?;
    if !src.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "." || name == ".." {
            continue;
        }
        let dst_path = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&entry.path(), &dst_path)?;
        } else {
            let content = read_file_limited(
                &entry.path(),
                MAX_GIT_OBJECT_BYTES + MAX_GIT_OBJECT_HEADER_BYTES,
                "git repository file",
            )?;
            fs::write(&dst_path, content)?;
        }
    }
    Ok(())
}

// ─── Entry point ────────────────────────────────────────────────────────────

pub fn main(args: Vec<OsString>) -> i32 {
    let str_args: Vec<String> = args
        .iter()
        .map(|a| a.to_string_lossy().to_string())
        .collect();
    match run(&str_args) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("fatal: {}", e);
            128
        }
    }
}

fn run(args: &[String]) -> io::Result<()> {
    let mut i = 1; // skip argv[0]
    let mut workdir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));

    // Parse global options
    while i < args.len() {
        if args[i] == "-C" {
            i += 1;
            if i >= args.len() {
                return Err(err("-C requires a directory argument"));
            }
            let p = PathBuf::from(&args[i]);
            workdir = if p.is_absolute() { p } else { workdir.join(p) };
            i += 1;
        } else if args[i] == "-c" {
            i += 1;
            if i >= args.len() {
                return Err(err("-c requires a config assignment"));
            }
            i += 1;
        } else {
            break;
        }
    }

    if i >= args.len() {
        eprintln!("usage: git <command> [<args>]");
        return Err(err("no subcommand"));
    }

    let subcmd = &args[i];
    let sub_args = &args[i + 1..];

    match subcmd.as_str() {
        "init" => {
            let mut path_arg = None;
            let mut quiet = false;
            for arg in sub_args {
                if arg == "-q" || arg == "--quiet" {
                    quiet = true;
                    continue;
                }
                if arg.starts_with('-') {
                    return Err(unsupported("init", &format!("does not support `{}`.", arg)));
                }
                if path_arg.replace(arg).is_some() {
                    return Err(err("usage: git init [<path>]"));
                }
            }
            let path = if let Some(path_arg) = path_arg {
                let p = PathBuf::from(path_arg);
                if p.is_absolute() {
                    p
                } else {
                    workdir.join(p)
                }
            } else {
                workdir
            };
            cmd_init(&path, quiet)
        }
        "add" => {
            if sub_args.is_empty() {
                return Err(err("nothing specified, nothing added"));
            }
            let paths: Vec<String> = sub_args.iter().map(|s| s.to_string()).collect();
            cmd_add(&workdir, &paths)
        }
        "commit" => {
            let mut message = None;
            let mut j = 0;
            while j < sub_args.len() {
                if sub_args[j] == "-m" && j + 1 < sub_args.len() {
                    message = Some(sub_args[j + 1].clone());
                    j += 2;
                } else {
                    j += 1;
                }
            }
            let msg = message.ok_or_else(|| err("no commit message (-m)"))?;
            cmd_commit(&workdir, &msg)
        }
        "branch" => cmd_branch(&workdir),
        "rev-parse" => cmd_rev_parse(&workdir, sub_args),
        "checkout" => {
            let mut create = false;
            let mut target = None;
            for arg in sub_args {
                if arg == "-b" {
                    create = true;
                } else if !arg.starts_with('-') {
                    target = Some(arg.clone());
                }
            }
            let t = target.ok_or_else(|| err("no branch name specified"))?;
            cmd_checkout(&workdir, &t, create)
        }
        "clone" => {
            if sub_args.is_empty() || sub_args.len() > 2 {
                return Err(err("usage: git clone <source> [<destination>]"));
            }
            let src_arg = &sub_args[0];
            if is_ssh_clone_source(src_arg) {
                return Err(unsupported(
                    "clone",
                    &format!("does not support SSH or git:// remotes (`{}`).", src_arg),
                ));
            }
            if has_http_auth(src_arg) {
                return Err(unsupported(
                    "clone",
                    &format!(
                        "does not support authenticated HTTP(S) remotes (`{}`).",
                        src_arg
                    ),
                ));
            }
            let dst_arg = if sub_args.len() == 2 {
                sub_args[1].clone()
            } else {
                infer_clone_destination(src_arg)?
            };
            let dst = PathBuf::from(&dst_arg);
            let dst = if dst.is_absolute() {
                dst
            } else {
                workdir.join(dst)
            };
            print_stdout_line(format_args!("Cloning into '{}'...", dst.display()))?;
            if is_remote_source(src_arg) {
                cmd_clone_remote(src_arg, &dst)
            } else {
                let src = PathBuf::from(src_arg);
                let src = if src.is_absolute() {
                    src
                } else {
                    workdir.join(src)
                };
                cmd_clone_local(&src, &dst)
            }
        }
        other => Err(unsupported(
            other,
            "is not implemented in the secure-exec VM git command.",
        )),
    }
}
