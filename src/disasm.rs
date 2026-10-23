use crate::chunk::{Chunk, Op};
use std::fmt::Write;

/// One instruction per line: byte offset, source line ("|" if same as previous), opcode, operand.
pub fn disassemble(chunk: &Chunk) -> String {
    let mut s = String::new();
    let mut ip = 0;
    while ip < chunk.code.len() {
        let op = Op::ALL[chunk.code[ip] as usize];
        if let Some(f) = chunk.functions.iter().find(|f| f.offset == ip) {
            writeln!(s, "== fn {} ==", f.name).unwrap();
        }
        // Innermost function whose body contains ip, for naming upvalues.
        let inside = chunk.functions.iter().filter(|f| f.offset <= ip && ip < f.end).max_by_key(|f| f.offset);
        let line = chunk.lines[ip];
        if ip > 0 && chunk.lines[ip - 1] == line {
            write!(s, "{ip:04}    | ").unwrap();
        } else {
            write!(s, "{ip:04} {line:4} ").unwrap();
        }
        write!(s, "{:<13}", format!("{op:?}")).unwrap();
        if op.operand_len() == 2 {
            let i = chunk.read_u16(ip + 1) as usize;
            match op {
                Op::Constant | Op::AddConst | Op::SubConst | Op::MulConst | Op::DivConst | Op::ModConst => {
                    write!(s, "{i:4}  ({})", chunk.show(chunk.constants[i]))
                }
                Op::GetLocal | Op::SetLocal | Op::Call | Op::MakeArray => write!(s, "{i:4}"),
                Op::GetUpvalue | Op::SetUpvalue => write!(s, "{i:4}  ({})", inside.map_or("?", |f| &f.upvalues[i].name)),
                Op::Closure => {
                    let f = &chunk.functions[i];
                    write!(s, "{i:4}  (<fn {}>)", f.name).unwrap();
                    for u in &f.upvalues {
                        let from = if u.is_local { "local" } else { "upvalue" };
                        write!(s, "\n            |   captures {from} {} ({})", u.index, u.name).unwrap();
                    }
                    Ok(())
                }
                Op::Jump | Op::JumpIfFalse | Op::JumpIfNotLt | Op::JumpIfNotLe | Op::JumpIfNotGt | Op::JumpIfNotGe => {
                    write!(s, "{i:4}  (-> {:04})", ip + 3 + i)
                }
                Op::Loop => write!(s, "{i:4}  (-> {:04})", ip + 3 - i),
                _ => write!(s, "{i:4}  ({})", chunk.global_names[i]),
            }
            .unwrap();
        }
        s.push('\n');
        ip += 1 + op.operand_len();
    }
    s
}

/// `dis --diff`: the two listings side by side, lined up on matching instructions, with a
/// marker between them: `<` only in the left (removed), `>` only in the right (added), `|`
/// a changed region where both sides have different instructions.
pub fn diff(left: &Chunk, right: &Chunk) -> String {
    let (l, r) = (disassemble(left), disassemble(right));
    let (l, r): (Vec<&str>, Vec<&str>) = (l.lines().collect(), r.lines().collect());
    // Compare instructions without their byte offset, and jumps without their offsets,
    // since removing anything shifts both.
    let key = |line: &str| -> String {
        let body = line.get(10..).unwrap_or(line).trim_end();
        match body.split_whitespace().next() {
            Some(op) if op.starts_with("Jump") || op == "Loop" => op.to_string(),
            _ => body.to_string(),
        }
    };
    let (lk, rk): (Vec<String>, Vec<String>) = (l.iter().map(|x| key(x)).collect(), r.iter().map(|x| key(x)).collect());

    // Longest common subsequence, then walk it to pair lines up.
    let mut lcs = vec![vec![0u32; rk.len() + 1]; lk.len() + 1];
    for i in (0..lk.len()).rev() {
        for j in (0..rk.len()).rev() {
            lcs[i][j] = if lk[i] == rk[j] { lcs[i + 1][j + 1] + 1 } else { lcs[i + 1][j].max(lcs[i][j + 1]) };
        }
    }
    let mut rows: Vec<(Option<&str>, Option<&str>)> = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < l.len() || j < r.len() {
        if i < l.len() && j < r.len() && lk[i] == rk[j] {
            rows.push((Some(l[i]), Some(r[j])));
            (i, j) = (i + 1, j + 1);
        } else if j == r.len() || (i < l.len() && lcs[i + 1][j] >= lcs[i][j + 1]) {
            rows.push((Some(l[i]), None));
            i += 1;
        } else {
            rows.push((None, Some(r[j])));
            j += 1;
        }
    }

    // Pair a run of removals with the run of additions right after it, so a replaced
    // region reads across instead of as two separate blocks.
    let mut paired: Vec<(Option<&str>, Option<&str>, char)> = Vec::new();
    let mut k = 0;
    while k < rows.len() {
        match rows[k] {
            (Some(a), Some(b)) => {
                paired.push((Some(a), Some(b), ' '));
                k += 1;
            }
            _ => {
                let start = k;
                while k < rows.len() && rows[k].1.is_none() {
                    k += 1;
                }
                let removed: Vec<&str> = rows[start..k].iter().filter_map(|r| r.0).collect();
                let add_start = k;
                while k < rows.len() && rows[k].0.is_none() {
                    k += 1;
                }
                let added: Vec<&str> = rows[add_start..k].iter().filter_map(|r| r.1).collect();
                for n in 0..removed.len().max(added.len()) {
                    let mark = match (removed.get(n), added.get(n)) {
                        (Some(_), Some(_)) => '|',
                        (Some(_), None) => '<',
                        _ => '>',
                    };
                    paired.push((removed.get(n).copied(), added.get(n).copied(), mark));
                }
            }
        }
    }

    let width = l.iter().map(|x| x.trim_end().len()).max().unwrap_or(0).max(4);
    let mut s = format!("{:width$}   {}\n", "-O0", "-O1");
    for (a, b, mark) in paired {
        writeln!(s, "{:width$} {mark} {}", a.unwrap_or("").trim_end(), b.unwrap_or("").trim_end()).unwrap();
    }
    s
}
