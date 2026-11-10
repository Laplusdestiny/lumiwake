//! 中身の近い画像をグループにまとめ、根拠（名前・サイズ・解像度）を付ける。

use super::hash::distance;
use super::names::similar_names;
use super::similarity_percent;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

/// 完全一致の読み比べで一度に読む大きさ
const CHUNK: usize = 64 * 1024;

/// 画像 1 枚の判定用の情報
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fingerprint {
    pub hash: u64,
    pub width: u32,
    pub height: u32,
    /// ファイルサイズ（バイト）
    pub size: u64,
}

/// グループ化の対象
#[derive(Debug, Clone)]
pub struct Candidate {
    /// セッションでの画像の番号
    pub index: usize,
    /// 今ある場所（名前の比較と、完全一致の読み比べに使う）
    pub path: PathBuf,
    pub print: Fingerprint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    /// 番号の小さい順
    pub members: Vec<Member>,
    /// 全員のバイト列が同じ
    pub exact: bool,
    pub same_size: bool,
    pub same_resolution: bool,
    /// 名前が似ている組が 1 つ以上ある
    pub similar_names: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub index: usize,
    /// グループ内で最も近い相手との類似度（%）
    pub similarity: u8,
    /// 画素数が単独で最大
    pub largest_resolution: bool,
    /// ファイルサイズが単独で最大
    pub largest_size: bool,
}

/// ハッシュの距離が `max_distance` 以下の画像をつないでグループにする（2 枚以上のものだけ）
pub fn group(candidates: &[Candidate], max_distance: u32, same_bytes: impl Fn(&Path, &Path) -> bool) -> Vec<Group> {
    let mut sorted: Vec<&Candidate> = candidates.iter().collect();
    sorted.sort_by_key(|c| c.index);
    let n = sorted.len();

    // 総当たりで比べ、近い組をつなぐ。あわせて各画像の「最も近い相手」との距離を覚える
    let mut parent: Vec<usize> = (0..n).collect();
    let mut nearest = vec![u32::MAX; n];
    for i in 0..n {
        for j in i + 1..n {
            let d = distance(sorted[i].print.hash, sorted[j].print.hash);
            if d <= max_distance {
                nearest[i] = nearest[i].min(d);
                nearest[j] = nearest[j].min(d);
                let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                // 根は常に番号の小さいほうにする（グループが先頭メンバーの順に並ぶ）
                parent[a.max(b)] = a.min(b);
            }
        }
    }

    let mut members_of: Vec<Vec<usize>> = vec![Vec::new(); n];
    for i in 0..n {
        let root = find(&mut parent, i);
        members_of[root].push(i);
    }
    members_of.into_iter().filter(|m| m.len() > 1).map(|m| describe(&sorted, &m, &nearest, &same_bytes)).collect()
}

fn find(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}

/// 1 グループ分の根拠と目印を求める。`members` は `sorted` での位置（番号順）
fn describe(
    sorted: &[&Candidate],
    members: &[usize],
    nearest: &[u32],
    same_bytes: &impl Fn(&Path, &Path) -> bool,
) -> Group {
    let of = |i: usize| sorted[i];
    let first = of(members[0]);
    let rest = &members[1..];
    let same_size = rest.iter().all(|&i| of(i).print.size == first.print.size);
    let same_resolution =
        rest.iter().all(|&i| (of(i).print.width, of(i).print.height) == (first.print.width, first.print.height));
    // サイズが違えば中身も違うので、読み比べるのはサイズが同じときだけ
    let exact = same_size && rest.iter().all(|&i| same_bytes(&first.path, &of(i).path));
    let names: Vec<String> = members.iter().map(|&i| crate::fileops::file_name(&of(i).path)).collect();
    let similar_names = names.iter().enumerate().any(|(i, a)| names[i + 1..].iter().any(|b| similar_names(a, b)));

    let pixels = |i: usize| u64::from(of(i).print.width) * u64::from(of(i).print.height);
    let largest_resolution = single_max(members, pixels);
    let largest_size = single_max(members, |i| of(i).print.size);
    Group {
        members: members
            .iter()
            .map(|&i| Member {
                index: of(i).index,
                similarity: similarity_percent(nearest[i]),
                largest_resolution: largest_resolution == Some(i),
                largest_size: largest_size == Some(i),
            })
            .collect(),
        exact,
        same_size,
        same_resolution,
        similar_names,
    }
}

/// 値が単独で最大のもの。最大が 2 つ以上あれば None
fn single_max(members: &[usize], value: impl Fn(usize) -> u64) -> Option<usize> {
    let max = members.iter().map(|&i| value(i)).max()?;
    let mut at_max = members.iter().filter(|&&i| value(i) == max);
    let first = at_max.next()?;
    at_max.next().is_none().then_some(*first)
}

/// 2 つのファイルの中身が同じか。読めなければ false
pub fn same_bytes(a: &Path, b: &Path) -> bool {
    fn open(p: &Path) -> std::io::Result<BufReader<File>> {
        Ok(BufReader::with_capacity(CHUNK, File::open(p)?))
    }
    let compare = || -> std::io::Result<bool> {
        let (mut a, mut b) = (open(a)?, open(b)?);
        loop {
            let (x, y) = (a.fill_buf()?, b.fill_buf()?);
            if x.is_empty() || y.is_empty() {
                return Ok(x.is_empty() && y.is_empty());
            }
            let n = x.len().min(y.len());
            if x[..n] != y[..n] {
                return Ok(false);
            }
            a.consume(n);
            b.consume(n);
        }
    };
    compare().unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::similar::similarity_percent;
    use std::cell::Cell;

    fn cand(index: usize, name: &str, hash: u64, (width, height): (u32, u32), size: u64) -> Candidate {
        Candidate { index, path: PathBuf::from("src").join(name), print: Fingerprint { hash, width, height, size } }
    }

    /// 名前・解像度・サイズがばらばらで、ハッシュだけ指定した候補
    fn plain(index: usize, hash: u64) -> Candidate {
        cand(index, &format!("x{}.jpg", index * 7 + 3), hash, (100 + index as u32, 100), 1000 + index as u64)
    }

    fn never(_: &Path, _: &Path) -> bool {
        false
    }

    fn indexes(g: &Group) -> Vec<usize> {
        g.members.iter().map(|m| m.index).collect()
    }

    #[test]
    fn groups_pairs_within_the_threshold_only() {
        // 0 と 1 は距離 6、2 は遠い
        let c = [plain(0, 0), plain(1, 0b11_1111), plain(2, u64::MAX)];
        let g = group(&c, 6, never);
        assert_eq!(g.len(), 1);
        assert_eq!(indexes(&g[0]), [0, 1]);
        assert!(group(&c, 5, never).is_empty(), "閾値より遠ければグループにしない");
        assert!(group(&c[..1], 6, never).is_empty(), "1 枚だけではグループにならない");
        assert!(group(&[], 6, never).is_empty());
    }

    #[test]
    fn chains_join_into_one_group_and_similarity_is_to_the_nearest() {
        // A-B 3、B-C 3、A-C 6
        let c = [plain(0, 0), plain(1, 0b111), plain(2, 0b11_1111)];
        let g = group(&c, 4, never);
        assert_eq!(g.len(), 1);
        assert_eq!(indexes(&g[0]), [0, 1, 2]);
        assert!(g[0].members.iter().all(|m| m.similarity == similarity_percent(3)));
    }

    #[test]
    fn groups_and_members_follow_session_order() {
        let far = 0xFFFF_FFFF_0000_0000;
        // 渡す順がばらばらでも番号順に並べる
        let c = [plain(5, far), plain(3, 0), plain(4, far | 1), plain(1, 1)];
        let g = group(&c, 2, never);
        assert_eq!(g.iter().map(indexes).collect::<Vec<_>>(), [vec![1, 3], vec![4, 5]]);
    }

    #[test]
    fn evidence_flags() {
        let c = [cand(0, "a.jpg", 0, (10, 10), 500), cand(1, "a (1).jpg", 0, (10, 10), 500)];
        let g = &group(&c, 0, never)[0];
        assert!(g.same_size && g.same_resolution && g.similar_names);

        let c = [cand(0, "a.jpg", 0, (10, 10), 500), cand(1, "zzz.jpg", 0, (20, 20), 900)];
        let g = &group(&c, 0, never)[0];
        assert!(!g.same_size && !g.same_resolution && !g.similar_names && !g.exact);

        // 3 枚のうち 1 組だけ名前が似ている。解像度は 1 枚だけ違う
        let c = [
            cand(0, "a.jpg", 0, (10, 10), 500),
            cand(1, "zzz.jpg", 0, (10, 10), 500),
            cand(2, "a_copy.jpg", 0, (20, 20), 500),
        ];
        let g = &group(&c, 0, never)[0];
        assert!(g.similar_names && g.same_size && !g.same_resolution);
    }

    #[test]
    fn exact_compares_bytes_only_when_sizes_match() {
        let calls = Cell::new(0);
        let count = |answer: bool| {
            let calls = &calls;
            move |_: &Path, _: &Path| {
                calls.set(calls.get() + 1);
                answer
            }
        };
        let different_sizes = [cand(0, "a.jpg", 0, (10, 10), 500), cand(1, "b.jpg", 0, (10, 10), 501)];
        assert!(!group(&different_sizes, 0, count(true))[0].exact);
        assert_eq!(calls.get(), 0, "サイズが違えば読まない");

        let same = [
            cand(0, "a.jpg", 0, (10, 10), 500),
            cand(1, "b.jpg", 0, (10, 10), 500),
            cand(2, "c.jpg", 0, (10, 10), 500),
        ];
        assert!(group(&same, 0, count(true))[0].exact);
        assert_eq!(calls.get(), 2, "先頭と残りを比べる");
        assert!(!group(&same, 0, count(false))[0].exact);
    }

    #[test]
    fn largest_marks_only_a_single_maximum() {
        let c = [
            cand(0, "a.jpg", 0, (10, 10), 900),
            cand(1, "b.jpg", 0, (40, 30), 500),
            cand(2, "c.jpg", 0, (20, 20), 500),
        ];
        let g = &group(&c, 0, never)[0];
        let marks: Vec<_> = g.members.iter().map(|m| (m.largest_resolution, m.largest_size)).collect();
        assert_eq!(marks, [(false, true), (true, false), (false, false)]);

        // 最大が 2 枚あるとき・全員同じときは付けない
        let c = [
            cand(0, "a.jpg", 0, (40, 30), 900),
            cand(1, "b.jpg", 0, (30, 40), 900),
            cand(2, "c.jpg", 0, (20, 20), 500),
        ];
        let g = &group(&c, 0, never)[0];
        assert!(g.members.iter().all(|m| !m.largest_resolution && !m.largest_size));
    }

    #[test]
    fn same_bytes_reads_files() {
        let dir = tempfile::tempdir().unwrap();
        let p = |name: &str| dir.path().join(name);
        let big = vec![7u8; 200_000];
        let mut big_other = big.clone();
        *big_other.last_mut().unwrap() = 8;
        std::fs::write(p("a"), &big).unwrap();
        std::fs::write(p("b"), &big).unwrap();
        std::fs::write(p("c"), &big_other).unwrap();
        std::fs::write(p("short"), &big[..100]).unwrap();
        assert!(same_bytes(&p("a"), &p("b")));
        assert!(!same_bytes(&p("a"), &p("c")), "最後の 1 バイトだけ違う");
        assert!(!same_bytes(&p("a"), &p("short")));
        assert!(!same_bytes(&p("a"), &p("missing")), "読めなければ完全一致としない");
    }
}
