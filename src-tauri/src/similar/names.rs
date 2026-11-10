//! ファイル名が似ているかの判定（複製で末尾に文字が足されたもの、連番）。

/// 拡張子つきのファイル名 2 つが、複製や連番らしい名前か
pub fn similar_names(a: &str, b: &str) -> bool {
    let (a, b) = (stem(a), stem(b));
    if a.is_empty() || b.is_empty() {
        return false;
    }
    a == b || appended(&a, &b) || appended(&b, &a) || sequential(&a, &b)
}

/// 拡張子を除いた名前（小文字）
fn stem(name: &str) -> String {
    let stem = match name.rfind('.') {
        Some(dot) if dot > 0 => &name[..dot],
        _ => name,
    };
    stem.to_lowercase()
}

/// `long` が、`short` の末尾に複製を表す文字（` (1)`、`_copy`、` - コピー`、`-2` など）を足した名前か
fn appended(short: &str, long: &str) -> bool {
    let Some(rest) = long.strip_prefix(short) else { return false };
    // `a` と `a1`、`cat` と `category` を同じ扱いにしないよう、区切りか括弧で始まるものだけ
    rest.starts_with([' ', '_', '-', '(']) && is_copy_suffix(rest)
}

/// 区切り・番号・括弧つき番号・copy・コピー だけでできているか
fn is_copy_suffix(mut rest: &str) -> bool {
    while !rest.is_empty() {
        let token = [" ", "_", "-", "copy", "コピー"].into_iter().find(|t| rest.starts_with(t)).map(str::len);
        let len = token.or_else(|| digits(rest)).or_else(|| {
            // (1) のような括弧つき番号
            let inner = rest.strip_prefix('(')?;
            let n = digits(inner)?;
            inner[n..].starts_with(')').then_some(n + 2)
        });
        match len {
            Some(len) => rest = &rest[len..],
            None => return false,
        }
    }
    true
}

/// 先頭の数字列の長さ
fn digits(s: &str) -> Option<usize> {
    let n = s.bytes().take_while(u8::is_ascii_digit).count();
    (n > 0).then_some(n)
}

/// 末尾の番号だけが 1 違う（連番）か
fn sequential(a: &str, b: &str) -> bool {
    match (split_number(a), split_number(b)) {
        (Some((pa, na)), Some((pb, nb))) => pa == pb && na.abs_diff(nb) == 1,
        _ => false,
    }
}

/// 末尾の数字列とその前の部分に分ける
fn split_number(s: &str) -> Option<(&str, u64)> {
    let start = s.len() - s.bytes().rev().take_while(u8::is_ascii_digit).count();
    Some((&s[..start], s[start..].parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn both(a: &str, b: &str) -> bool {
        let r = similar_names(a, b);
        assert_eq!(r, similar_names(b, a), "{a} / {b}: 向きで結果が変わる");
        r
    }

    #[test]
    fn appended_suffixes_match() {
        for (a, b) in [
            ("a.jpg", "a (1).jpg"),
            ("a.jpg", "a_copy.jpg"),
            ("a.jpg", "a - コピー.jpg"),
            ("a.jpg", "a-2.jpg"),
            ("photo.png", "photo copy 2.png"),
            ("A.JPG", "a (1).jpg"),
            ("a.jpg", "a - コピー (2).jpg"),
            ("a.jpg", "a(1).jpg"),
        ] {
            assert!(both(a, b), "{a} / {b}");
        }
    }

    #[test]
    fn sequential_numbers_match() {
        for (a, b) in [("IMG_0012.jpg", "IMG_0013.jpg"), ("shot9.png", "shot10.png"), ("12.jpg", "13.jpg")] {
            assert!(both(a, b), "{a} / {b}");
        }
    }

    #[test]
    fn same_stem_with_another_extension_matches() {
        assert!(both("a.jpg", "a.png"));
        assert!(both("a.jpg", "a.jpg"));
        assert!(both("noext", "NOEXT"));
    }

    #[test]
    fn unrelated_names_do_not_match() {
        for (a, b) in [
            ("a.jpg", "b.jpg"),
            ("IMG_0012.jpg", "IMG_0014.jpg"),
            ("cat.jpg", "category.jpg"),
            ("a.jpg", "ab.jpg"),
            ("IMG_0012.jpg", "DSC_0013.jpg"),
            ("a.jpg", "a1.jpg"),
            ("a.jpg", "a - 旅行.jpg"),
            ("", "a.jpg"),
        ] {
            assert!(!both(a, b), "{a} / {b}");
        }
    }
}
