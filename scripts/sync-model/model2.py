#!/usr/bin/env python3
"""Executable model of the sync rules. Standard library only. Run with python3 -I.

Entry keys:
  ('add', H, d)  value = record (name, notes, rating, status, tags, folders)
  ('del', H)     value = None for a deletion, or with the rule undo_revive
                 ('u', before, rec) for the undo of a deletion: the cut goes back
                 to `before`, the cut that the undoing device had just before its
                 deletion, and `rec` is the item as that device had it
  ('floor', H)   value = a time (rule compact). Entries of H that are not later
                 than it are dropped and never count again. The larger value wins.
  ('name'|'notes'|'rating'|'status', H)  value
  ('it', H, T)   value 'add' | 'remove'      (item_tag)
  ('fi', H, F)   value 'add' | 'remove'      (folder_item)
  ('td', T)      tag_delete
  ('fd', F)      folder_delete
An entry is (time, value); time = (wall, counter, device id).
"""
import sys
import random
import argparse
import multiprocessing

ZERO = (0, 0, '')
ALL_FILES = ('h1', 'h2', 'h3')
TAGS = ('t1', 't2')
FOLDERS = ('f1',)
NAMES = ('nA', 'nB', 'nC')
NOTES = ('', 'x', 'y')
RATINGS = (0, 1, 2)
STATUSES = ('inbox', 'active', 'trash')
SCALARS = ('name', 'notes', 'rating', 'status')
EMPTY = {'notes': '', 'rating': 0}
DEFAULTS = ('', '', 0, 'inbox')
POSTS = ('p1', 'p2')
KEEP = 5          # keep time before an original may be purged, in units of world time
COMPACT = 5       # time after a cut before a device drops the entries it made obsolete
MAXDEV = 5
MAXIDS = 9


class Rules:
    """Rule switches. All False / 'split' = the rules as given (adversarial reading)."""
    NAMES = ('upload_first', 'join_republish', 'task_keep', 'restore_merge', 'restore_newid',
             'purge_published', 'purge_lock', 'tagdel_baseline', 'nopins',
             # second round
             'copy_published',     # V1: a copy holds only entries that are in the shared folder
             'disk_restore',       # V2: a restore keeps the files on disk; 'disklost' exists
             'restore_merge_all',  # V3: a restored device merges everything before it acts
             'orphan_cleanup',     # variant: a device may delete files its view does not show
             'nodisklost',         # switch the disk-lost action off (to separate causes)
             'keep_deleted',       # a deleted file stays on disk until its delete is published
             'collections', 'one_tick', 'member_del', 'coll_delreg', 'cadd_once',
             # third round
             'undo_revive',        # the undo of a deletion moves the cut back (('del', H) values)
             'compact',            # devices drop entries that a cut made obsolete, with floors
             'compact_nofloor',    # variant: drop them without writing a floor
             'compact_unpublished',  # variant: a floor may rest on an unpublished deletion
             'compact_own_later')  # a device drops its own entries only after its floor is published

    def __init__(self, on=()):
        for n in self.NAMES:
            setattr(self, n, n in on)

    def __repr__(self):
        return 'Rules(%s)' % ','.join(n for n in self.NAMES if getattr(self, n))


# ---------------------------------------------------------------- view

def is_undo(e):
    return e is not None and isinstance(e[1], tuple) and e[1][0] == 'u'


def floor_of(held, X):
    e = held.get(('floor', X))
    return e[1] if e else ZERO


def del_time(held, H):
    """The cut of H: an entry of H counts only when it is later than the cut. It is
    the time of the held deletion, or the time an undo went back to, and never
    earlier than the floor."""
    e = held.get(('del', H))
    if e is None:
        c = ZERO
    elif is_undo(e):
        c = e[1][1]
    else:
        c = e[0]
    return max(c, floor_of(held, H))


def live_adds(held, ids, H):
    """The cut and the sorted adds of H that count. When an undo is held and no add
    counts, the record of the undo counts as an add at the time of the undo."""
    dt = del_time(held, H)
    live = []
    for i in ids:
        e = held.get(('add', H, i))
        if e is not None and e[0] > dt:
            live.append(e)
    if not live:
        e = held.get(('del', H))
        if is_undo(e):
            live.append((e[0], e[1][2]))
    live.sort()
    return dt, live


def exists(held, ids, H):
    return bool(live_adds(held, ids, H)[1])


def _fields(held, live, X, dt, rules):
    """Scalars, tags and folders of X (a file H or a collection K) from the sorted live
    records and the held field entries later than dt."""
    out = []
    for idx, f in enumerate(SCALARS):
        e = held.get((f, X))
        if e is not None and e[0] > dt:
            v = e[1]
        elif not live:
            v = DEFAULTS[idx]
        elif f in EMPTY:
            v = EMPTY[f]
            for _, r in live:
                if r[idx] != v:
                    v = r[idx]
                    break
        else:
            v = live[0][1][idx]
        out.append(v)
    tags = []
    for T in TAGS:
        e = held.get(('it', X, T))
        if e is not None and e[0] > dt:
            if e[1] == 'remove':
                continue
            mt = e[0]
        else:
            mt = None
            for t, r in live:
                if T in r[4]:
                    # rule change (tagdel_baseline): a record written at the start of
                    # recording carries membership time zero.
                    if rules is not None and rules.tagdel_baseline and r[-1] == 'baseline':
                        if mt is None:
                            mt = ZERO
                    else:
                        mt = t
            if mt is None:
                continue
        td = held.get(('td', T))
        if td is not None and td[0] >= mt:
            continue
        tags.append(T)
    folders = []
    for F in FOLDERS:
        if ('fd', F) in held:
            continue
        e = held.get(('fi', X, F))
        if e is not None and e[0] > dt:
            if e[1] == 'remove':
                continue
        else:
            if not any(F in r[5] for _, r in live):
                continue
        folders.append(F)
    out.append(frozenset(tags))
    out.append(frozenset(folders))
    return out


def item(held, ids, H, rules=None):
    """The item H as the view shows it, or None when H does not exist. The seventh
    field is the collection the file is a member of, or None when it is loose."""
    dt, live = live_adds(held, ids, H)
    if not live:
        return None
    out = _fields(held, live, H, dt, rules)
    coll, jt = None, None
    m = held.get(('mem', H))
    if m is not None and (m[0] > dt or rules is None or not rules.member_del):
        coll, jt = m[1], m[0]
    else:
        for t, r in live:
            if r[6] is not None:              # earliest live add that names a collection
                coll, jt = r[6], t
                break
    if coll is not None and rules is not None and rules.coll_delreg:
        cd = held.get(('cdel', coll))
        if cd is not None and cd[0] >= jt:
            coll = None                       # joined before the collection was deleted
    out.append(coll)
    return tuple(out)


def coll_fields(held, ids, K, rules=None):
    """Fields of collection K. Every held collection_add(K, d) is live."""
    live = sorted(e for e in (held.get(('cadd', K, i)) for i in ids) if e is not None)
    return _fields(held, live, K, ZERO, rules)


# ---------------------------------------------------------------- state

class Dev:
    __slots__ = ('id', 'rec', 'pre', 'held', 'own', 'files', 'tasks', 'done', 'clk',
                 'skew', 'stash', 'delst', 'snap', 'cand', 'blocked', 'cleared', 'left',
                 'off', 'removed')

    def __init__(self, id):
        self.id = id
        self.rec = True        # recording
        self.pre = {}          # items held before recording started: H -> record
        self.held = {}         # key -> entry
        self.own = {}          # entries this device publishes
        self.files = set()     # files physically on the device
        self.tasks = []        # unfinished tasks: (files {H: record}, created set)
        self.done = []         # ended tasks: tuple of H for which the task wrote an add
        self.clk = (0, 0)
        self.skew = 0
        self.stash = {}        # H -> record kept for Undo of Delete Permanently
        self.delst = {}        # H -> world time when the held delete(H) was stored
        self.snap = None       # backup for restore / join
        self.cand = None       # purge candidates between purge_begin and purge_end
        self.blocked = False   # restored and has not merged its own published entries yet
        self.cleared = set()   # (key, time) of adds checked against purge markers
        self.left = set()      # files on disk that the view does not show (after a restore)
        self.off = 0           # world time until which the device neither publishes nor merges
        self.removed = set()   # own keys dropped by compaction and not yet removed from pub

    def clone(self):
        d = Dev(self.id)
        d.rec = self.rec
        d.pre = self.pre
        d.held = dict(self.held)
        d.own = dict(self.own)
        d.files = set(self.files)
        d.tasks = [(dict(f), set(c)) for f, c in self.tasks]
        d.done = list(self.done)
        d.clk = self.clk
        d.skew = self.skew
        d.stash = dict(self.stash)
        d.delst = dict(self.delst)
        d.snap = self.snap
        d.cand = None if self.cand is None else set(self.cand)
        d.blocked = self.blocked
        d.cleared = set(self.cleared)
        d.left = set(self.left)
        d.off = self.off
        d.removed = set(self.removed)
        return d

    def key(self):
        return (self.id, self.rec, tuple(sorted(self.held.items())),
                tuple(sorted(self.own.items())), tuple(sorted(self.files)),
                tuple((tuple(sorted(f.items())), tuple(sorted(c))) for f, c in self.tasks),
                tuple(self.done), self.clk, self.skew, tuple(sorted(self.stash.items())),
                tuple(sorted(self.delst.items())), self.snap is not None and self.snap[5],
                None if self.cand is None else tuple(sorted(self.cand)), self.blocked,
                tuple(sorted(self.cleared)), tuple(sorted(self.left)), self.off,
                tuple(sorted(self.removed)))


class World:
    def __init__(self, rules, init, nfiles=2):
        """init: tuple per device of (skew, pre) where pre is None (empty, recording)
        or a tuple of (H, record) (holds these items, not yet recording)."""
        self.rules = rules
        self.files = ALL_FILES[:nfiles]
        self.devs = []
        self.ids = []
        self.pub = {}       # device id -> {key: entry}: the entries part of the shared folder
        self.orig = set()   # originals in the shared folder
        self.locks = {}     # H -> device id: purge markers (purge_lock rule only)
        self.pins = set()   # (H, device id): publish markers (purge_lock rule only)
        self.now = 10
        self.log = []       # ghost log of user actions: (kind, key, time, value, dev, src)
        self.lost = set()   # indexes of logged actions whose entry a restore destroyed
        self.tr = None      # per-step trace of written entries when tracing
        self.snapseq = 0
        self.aseq = 0       # number of the current local action
        self.cur = None     # the time of the current action (one_tick rule)
        self.gseq = 0       # random collection keys made so far
        self.colls = set()  # every collection key that was used
        self.shown = []     # ghost: (K, members, action) when a device showed K after creating it
        self.prec = {}      # device index -> collections held before recording
        self.dropped = {}   # ghost: device id -> the newest entry per key it dropped
        for i, entry in enumerate(init):
            skew, pre = entry[0], entry[1]
            d = Dev('d%d' % (i + 1))
            d.skew = skew
            if pre is not None:
                d.rec = False
                d.pre = dict(pre)
                d.files = set(d.pre)
                if len(entry) > 2 and entry[2]:
                    self.prec[i] = tuple(entry[2])
            self.devs.append(d)
            self.ids.append(d.id)

    def clone(self):
        w = World.__new__(World)
        w.rules = self.rules
        w.files = self.files
        w.devs = [d.clone() for d in self.devs]
        w.ids = list(self.ids)
        w.pub = {k: dict(v) for k, v in self.pub.items()}
        w.orig = set(self.orig)
        w.locks = dict(self.locks)
        w.pins = set(self.pins)
        w.now = self.now
        w.log = list(self.log)
        w.lost = set(self.lost)
        w.tr = None
        w.snapseq = self.snapseq
        w.aseq, w.cur, w.gseq = self.aseq, None, self.gseq
        w.colls, w.shown, w.prec = set(self.colls), list(self.shown), self.prec
        w.dropped = {k: dict(v) for k, v in self.dropped.items()}
        return w

    def key(self):
        return (tuple(d.key() for d in self.devs),
                tuple(sorted((k, tuple(sorted(v.items()))) for k, v in self.pub.items())),
                tuple(sorted(self.orig)), self.now, tuple(self.log),
                tuple(sorted(self.locks.items())), tuple(sorted(self.lost)),
                tuple(sorted(self.pins)), self.gseq, tuple(self.shown),
                tuple(sorted((k, tuple(sorted(v.items()))) for k, v in self.dropped.items())))

    # ------------------------------------------------------------ clock and entries

    def tick(self, d):
        w = self.now + d.skew
        l, c = d.clk
        d.clk = (w, 0) if w > l else (l, c + 1)
        return (d.clk[0], d.clk[1], d.id)

    def begin_action(self):
        self.aseq += 1
        self.cur = None

    def write(self, d, key, value, kind, src=None):
        if self.rules.one_tick:
            # rule change: every entry of one local action carries the same time
            if self.cur is None:
                self.cur = self.tick(d)
            t = self.cur
        else:
            t = self.tick(d)
        e = (t, value)
        d.held[key] = e
        d.own[key] = e
        if key[0] == 'del':
            d.delst[key[1]] = self.now
        self.log.append((kind, key, t, value, d.id, src, self.aseq))
        if self.tr is not None:
            self.tr.append('%s@%s' % (fmt_key(key, value), fmt_time(t)))
        return t

    def ex(self, d, H):
        return exists(d.held, self.ids, H)

    def it(self, d, H):
        return item(d.held, self.ids, H, self.rules)

    def view(self, d):
        """The library the device shows: items, and collections with fields and members."""
        return self.view_held(d.held)

    def view_held(self, held):
        items = {}
        for H in self.files:
            it = item(held, self.ids, H, self.rules)
            if it is not None:
                items[H] = it
        colls = {}
        if self.rules.collections:
            mem = {}
            for H, it in items.items():
                if it[6] is not None:
                    mem.setdefault(it[6], []).append(H)
            for K, hs in mem.items():
                o = held.get(('ord', K))
                lst = []
                for H in (o[1] if o else ()):
                    if H in hs and H not in lst:
                        lst.append(H)
                lst += sorted(H for H in hs if H not in lst)
                colls[K] = tuple(coll_fields(held, self.ids, K, self.rules)) + (tuple(lst),)
        return items, colls

    def kof(self, d, K):
        """Resolve a collection name; '@h1' means the collection h1 is a member of."""
        if K.startswith('@'):
            it = self.it(d, K[1:])
            return None if it is None else it[6]
        return K

    def protected(self, d, H):
        for _, created in d.tasks:
            if H in created:
                return True
        return False

    def gc(self, d):
        """A device drops the file of an H that does not exist in its view, except a
        file created by an unfinished task."""
        for H in list(d.files):
            if not self.ex(d, H) and not self.protected(d, H):
                d.files.discard(H)
        for H in list(d.left):
            if self.ex(d, H):                 # the view shows H again: held without a fetch
                d.left.discard(H)
                d.files.add(H)

    # ------------------------------------------------------------ local actions

    def start(self, d):
        if d.rec:
            return False
        d.rec = True
        prec = self.prec.get(self.devs.index(d), ()) if self.rules.collections else ()
        prec = [(K, c) for K, c in prec if any(r[6] == K for r in d.pre.values())]
        for K, crec in prec:
            if self.rules.tagdel_baseline:
                crec = crec + ('baseline',)
            self.colls.add(K)
            self.write(d, ('cadd', K, d.id), crec, 'cadd', 'start')
        for H in sorted(d.pre):
            rec = d.pre[H]
            if not self.rules.collections:
                rec = rec[:6] + (None,)
            if self.rules.tagdel_baseline:
                rec = rec + ('baseline',)
            self.write(d, ('add', H, d.id), rec, 'add', 'start')
        self.note_shown(d, [K for K, _ in prec])
        return True

    # ------------------------------------------------------------ collections

    def note_shown(self, d, keys):
        if not keys:
            return
        items, colls = self.view(d)
        for K in keys:
            if K in colls:
                self.shown.append((K, colls[K][6], self.aseq))

    def import_post(self, d, K, items, crec):
        """Import of a post with several files into the collection K of the post."""
        if not d.rec or not self.rules.collections:
            return False
        existed = K in self.view(d)[1]
        todo = []
        for H, rec in items:
            if H not in self.files:
                continue
            it = self.it(d, H)
            if it is None:
                todo.append((H, 'add', rec))
            elif it[6] is None:
                todo.append((H, 'take', None))
            elif H not in d.files:
                todo.append((H, 'hold', None))
        if not todo:
            return False
        self.colls.add(K)
        if not existed and any(x[1] != 'hold' for x in todo) and not (
                self.rules.cadd_once and ('cadd', K, d.id) in d.held):
            # rule change (cadd_once): a device never replaces its own collection_add
            self.write(d, ('cadd', K, d.id), crec, 'cadd', 'ipost')
        for H, what, rec in todo:
            if what == 'add':
                self.write(d, ('add', H, d.id), rec[:6] + (K,), 'add', 'import')
                d.left.discard(H)
            elif what == 'take':
                self.write(d, ('mem', H), K, 'mem')
            d.files.add(H)
        self.note_shown(d, [K])
        return True

    def group(self, d, K, hs, crec):
        if not d.rec or not self.rules.collections or len(hs) < 2 or K in self.colls:
            return False
        for H in hs:
            it = self.it(d, H)
            if it is None or it[6] is not None:
                return False
        self.colls.add(K)
        self.gseq += 1
        self.write(d, ('cadd', K, d.id), crec, 'cadd', 'group')
        for H in hs:
            self.write(d, ('mem', H), K, 'mem')
        self.write(d, ('ord', K), tuple(hs), 'ord')
        self.note_shown(d, [K])
        return True

    def ungroup(self, d, K):
        if not d.rec or not self.rules.collections:
            return False
        K = self.kof(d, K)
        c = self.view(d)[1].get(K) if K else None
        if c is None:
            return False
        for H in c[6]:
            self.write(d, ('mem', H), None, 'mem')
        return True

    def detach(self, d, H):
        if not d.rec or not self.rules.collections:
            return False
        it = self.it(d, H)
        if it is None or it[6] is None:
            return False
        self.write(d, ('mem', H), None, 'mem')
        return True

    def cdelete(self, d, K):
        """Delete a collection permanently: delete(H) for every member the view shows."""
        if not d.rec or not self.rules.collections:
            return False
        K = self.kof(d, K)
        c = self.view(d)[1].get(K) if K else None
        if c is None:
            return False
        key = t = None
        for H in c[6]:
            key = ('del', H)
            t = self.write(d, key, None, 'del')
        if self.rules.coll_delreg:
            key = ('cdel', K)
            t = self.write(d, key, None, 'cdel')
        # ghost record of the action, tied to the last entry it wrote
        self.log.append(('cdact', key, t, K, d.id, None, self.aseq))
        self.gc(d)
        return True

    def reorder(self, d, K, lst):
        if not d.rec or not self.rules.collections:
            return False
        K = self.kof(d, K)
        c = self.view(d)[1].get(K) if K else None
        if c is None:
            return False
        new = [H for H in lst if H in c[6]]
        new += [H for H in c[6] if H not in new]
        if tuple(new) == c[6]:
            return False
        self.write(d, ('ord', K), tuple(new), 'ord')
        return True

    def cedit(self, d, K, f, v):
        if not d.rec or not self.rules.collections:
            return False
        K = self.kof(d, K)
        c = self.view(d)[1].get(K) if K else None
        if c is None or c[SCALARS.index(f)] == v:
            return False
        self.write(d, (f, K), v, 'edit')
        return True

    def ctag(self, d, K, T, op):
        if not d.rec or not self.rules.collections:
            return False
        K = self.kof(d, K)
        c = self.view(d)[1].get(K) if K else None
        if c is None or (T in c[4]) == (op == 'add'):
            return False
        self.write(d, ('it', K, T), op, 'tag')
        return True

    def cfolder(self, d, K, F, op):
        if not d.rec or not self.rules.collections or ('fd', F) in d.held:
            return False
        K = self.kof(d, K)
        c = self.view(d)[1].get(K) if K else None
        if c is None or (F in c[5]) == (op == 'add'):
            return False
        self.write(d, ('fi', K, F), op, 'fold')
        return True

    def do_import(self, d, H, rec):
        if not d.rec or H not in self.files:
            return False
        if not self.ex(d, H):
            self.write(d, ('add', H, d.id), rec, 'add', 'import')
            d.files.add(H)
            d.left.discard(H)
            return True
        if H in d.files:
            return False
        d.files.add(H)
        return True

    def do_delete(self, d, H):
        if not d.rec or H not in d.files:
            return False
        it = self.it(d, H)
        if it is None:
            return False
        before = del_time(d.held, H)
        t = self.write(d, ('del', H), None, 'del')
        d.stash[H] = (it, t, before)
        self.gc(d)
        return True

    def do_undo(self, d, H):
        if H not in d.stash:
            return False
        rec, deleted, before = d.stash.pop(H)
        if self.rules.undo_revive:
            # rule change: the undo goes into the deletion's place and moves the cut
            # back to where it was on this device just before its deletion
            self.write(d, ('del', H), ('u', before, rec), 'undo', deleted)
        else:
            self.write(d, ('add', H, d.id), rec, 'add', ('undo', deleted))
        d.files.add(H)
        return True

    def do_edit(self, d, H, f, v):
        if not d.rec:
            return False
        it = self.it(d, H)
        if it is None or it[SCALARS.index(f)] == v:
            return False
        self.write(d, (f, H), v, 'edit')
        return True

    def do_tag(self, d, H, T, op):
        if not d.rec:
            return False
        it = self.it(d, H)
        if it is None or (T in it[4]) == (op == 'add'):
            return False
        self.write(d, ('it', H, T), op, 'tag')
        return True

    def do_folder(self, d, H, F, op):
        if not d.rec or ('fd', F) in d.held:
            return False
        it = self.it(d, H)
        if it is None or (F in it[5]) == (op == 'add'):
            return False
        self.write(d, ('fi', H, F), op, 'fold')
        return True

    def do_tagdel(self, d, T):
        if not d.rec:
            return False
        self.write(d, ('td', T), None, 'td')
        return True

    def do_folderdel(self, d, F):
        if not d.rec or ('fd', F) in d.held:
            return False
        self.write(d, ('fd', F), None, 'fd')
        return True

    # ------------------------------------------------------------ import tasks

    def task_begin(self, d, items):
        if not d.rec or len(d.tasks) >= 2:
            return False
        files, created = {}, set()
        for H, rec in items:
            if H not in self.files:
                continue
            if H in d.files and self.ex(d, H):
                continue                      # duplicate of a held item
            files[H] = rec
            if H not in d.files:
                d.files.add(H)
                created.add(H)
        if not files:
            return False
        d.tasks.append((files, created))
        return True

    def task_end(self, d, i):
        if i >= len(d.tasks):
            return False
        files, created = d.tasks.pop(i)
        wrote = []
        for H in sorted(files):
            if not self.ex(d, H):
                if self.rules.task_keep and H not in d.files:
                    continue                  # rule change: no add without the file
                self.write(d, ('add', H, d.id), files[H], 'add', 'tend')
                wrote.append(H)
        d.done.append(tuple(wrote))
        self.gc(d)
        return True

    def task_undo(self, d, i):
        if i >= len(d.tasks):
            return False
        files, created = d.tasks.pop(i)
        for H in created:
            if self.rules.task_keep:
                if self.ex(d, H):
                    continue
                other = [t for t in d.tasks if H in t[0]]
                if other:
                    other[0][1].add(H)        # the other unfinished task now owns the file
                    continue
            else:
                dt = del_time(d.held, H)
                if any((e := d.held.get(('add', H, o))) is not None and e[0] > dt
                       for o in self.ids if o != d.id):
                    continue                  # exists by another device's add
            d.files.discard(H)
        return True

    def task_undo_after(self, d, j):
        if j >= len(d.done):
            return False
        items = d.done.pop(j)
        for H in items:
            it = self.it(d, H)
            if it is not None and it[3] != 'trash':
                self.write(d, ('status', H), 'trash', 'edit')
        return True

    # ------------------------------------------------------------ shared folder

    def upload(self, d):
        if not d.rec:
            return False
        ch = False
        for H in sorted(d.files):
            if H not in self.orig and self.ex(d, H):
                if self.rules.purge_lock and self.locks.get(H, d.id) != d.id:
                    continue                  # another device's purge is in progress
                self.orig.add(H)
                ch = True
        return ch

    def unpublished_adds(self, d):
        p = self.pub.get(d.id, {})
        return [k for k, e in d.own.items() if (k[0] == 'add' or is_undo(e)) and k[1] in d.files
                and ((cur := p.get(k)) is None or e[0] > cur[0])]

    def pin(self, d):
        """purge_lock rule: before a device publishes an add of a file it holds, it
        writes a publish marker for H into the shared folder."""
        if not d.rec or not self.rules.purge_lock:
            return False
        ch = False
        for k in self.unpublished_adds(d):
            if (k[1], d.id) not in self.pins:
                self.pins.add((k[1], d.id))
                ch = True
        return ch

    def publish(self, d, pick=None):
        """Writes the device's entries into the shared folder. With `pick`, the
        publish stops part way: only the keys for which pick(key) is true are
        written, which is what a sync that writes one file at a time and is
        interrupted leaves behind."""
        if not d.rec or self.away(d):
            return False
        ch = False
        p = self.pub.get(d.id, {})
        for k in sorted(d.removed):
            if pick is not None and not pick(k):
                continue
            # the files of the device are written again without what compaction
            # dropped; a published entry that still counts stays
            pe = p.get(k)
            if pe is not None and k not in d.own and not pe[0] > self.dead_line(d, k[1]):
                del p[k]
                ch = True
            d.removed.discard(k)
        if self.rules.upload_first and not self.rules.purge_lock:
            ch = self.upload(d) or ch
        p = self.pub.get(d.id, {})
        for k, e in d.own.items():
            if pick is not None and not pick(k):
                continue
            cur = p.get(k)
            if cur is not None and e[0] <= cur[0]:
                continue
            if (k[0] == 'add' or is_undo(e)) and k[1] in d.files:
                # an undo makes the file exist like an add, so it waits for the
                # original in the same way
                H = k[1]
                if self.rules.purge_lock:
                    if (k, e[0]) not in d.cleared:
                        COUNTS['add_waits'] += 1
                        continue              # not checked against purge markers yet
                elif self.rules.upload_first and H not in self.orig:
                    continue                  # the add waits until its original is uploaded
            p[k] = e
            ch = True
        if p:
            self.pub[d.id] = p
        if self.rules.purge_lock and pick is None:
            d.cleared.clear()
            keep = set(k[1] for k in self.unpublished_adds(d))
            for x in [x for x in self.pins if x[1] == d.id and x[0] not in keep]:
                self.pins.discard(x)
        return ch

    def clear(self, d):
        """purge_lock rule, second step of publishing an add: with its publish marker in
        place the device checks that no other device's purge marker for H is present,
        uploads the original when it is missing, and only then may publish the add."""
        if not d.rec or not self.rules.purge_lock:
            return False
        ch = False
        for k in self.unpublished_adds(d):
            H, t = k[1], d.own[k][0]
            if (k, t) in d.cleared or (H, d.id) not in self.pins:
                continue
            if self.locks.get(H, d.id) != d.id:
                COUNTS['marker_blocks_add'] += 1
                continue
            self.orig.add(H)
            d.cleared.add((k, t))
            ch = True
        return ch

    def droppable(self, key):
        """Whether the entry kind of key is one that a cut of key[1] makes obsolete."""
        return key[0] in DROPPABLE or key[0] == 'mem' and self.rules.member_del

    def merge(self, d, src, keys=None, gc=True):
        if not d.rec or self.away(d):
            return False
        if d.blocked == 2 and gc:
            return False                      # V3: its first merge is a merge of everything
        p = self.pub.get(src)
        if not p:
            if src == d.id and d.blocked == 1:
                d.blocked = False
                return True
            return False
        ch = False
        for k in (list(p) if keys is None else keys):
            e = p.get(k)
            if e is None:
                continue
            t = e[0]
            if (t[0], t[1]) > d.clk:
                d.clk = (t[0], t[1])
                ch = True
            cur = d.held.get(k)
            if k[0] == 'floor':
                newer = cur is None or (e[1], t) > (cur[1], cur[0])
            else:
                newer = cur is None or t > cur[0]
                if newer and self.rules.compact and self.droppable(k) and \
                        not t > floor_of(d.held, k[1]):
                    newer = False             # below the floor: it never counts again
                    COUNTS['stored_below_floor_skipped'] += 1
                if newer and is_undo(e) and e[1][1] < floor_of(d.held, k[1]):
                    COUNTS['undo_arrives_after_floor'] += 1
            if newer:
                d.held[k] = e
                ch = True
                if k[0] == 'del':
                    d.delst[k[1]] = self.now
            if t[2] == d.id or self.rules.join_republish and k in d.own:
                cur = d.own.get(k)
                if cur is None or t > cur[0]:
                    d.own[k] = e
                    ch = True
        if src == d.id and keys is None and d.blocked == 1:
            d.blocked = False
            ch = True
        if ch and gc:
            self.gc(d)
        return ch

    def fetch(self, d, H):
        if not d.rec or H in d.files or not self.ex(d, H):
            return False
        if H not in self.orig:
            COUNTS['fetch_waits'] += 1
            return False
        d.files.add(H)
        COUNTS['fetches'] += 1
        return True

    # ------------------------------------------------------------ compaction

    def away(self, d):
        return self.now < d.off

    def dead_line(self, d, X):
        """Entries of X that are not later than this never count again on d."""
        if self.rules.compact_nofloor:
            return del_time(d.held, X)
        return floor_of(d.held, X)

    def compact(self, d):
        """The device drops the entries of each file that its cut made obsolete, once
        the cut is COMPACT old by the device's wall clock. It first writes the cut as
        the floor of the file, so that every other device drops the same entries and
        an undo that arrives later cannot bring them back."""
        if not d.rec or not (self.rules.compact or self.rules.compact_nofloor):
            return False
        ch = False
        for H in self.files:
            c = del_time(d.held, H)
            if self.rules.compact and c > floor_of(d.held, H) and \
                    self.now + d.skew - c[0] >= COMPACT and self.cut_published(d, H):
                self.write(d, ('floor', H), c, 'floor')
                COUNTS['floors_written'] += 1
                ch = True
            if self.rules.compact_nofloor and self.now + d.skew - c[0] < COMPACT:
                continue
            line = self.dead_line(d, H)
            own_too = not self.rules.compact_own_later or self.floor_published(d, H)
            for k in [k for k in d.held if k[1] == H and self.droppable(k)]:
                e = d.held[k]
                if e[0] > line:
                    continue
                if not own_too and d.own.get(k) is not None:
                    continue              # it waits until the floor is in the shared folder
                mine = self.dropped.setdefault(d.id, {})
                g = mine.get(k)
                if g is None or e[0] > g[0]:
                    mine[k] = e
                del d.held[k]
                COUNTS['entries_dropped'] += 1
                ch = True
                o = d.own.get(k)
                if o is not None and not o[0] > line:
                    del d.own[k]
                    d.removed.add(k)
        return ch

    def cut_published(self, d, H):
        """Whether the entry that sets the cut of H is in the shared folder: it came
        from another device, or the device has published it. A floor without that
        entry would end the entries of H for good while a lost disk or a restore
        can still lose the deletion that justified it."""
        e = d.held.get(('del', H))
        if e is None:
            return False
        if self.rules.compact_unpublished or e[0][2] != d.id:
            return True
        pe = self.pub.get(d.id, {}).get(('del', H))
        return pe is not None and pe[0] >= e[0]

    def floor_published(self, d, H):
        """Whether the floor that the device holds for H is in the shared folder: it
        came from another device, or the device has published it."""
        f = d.held.get(('floor', H))
        if f is None:
            return False
        if f[0][2] != d.id:
            return True
        pe = self.pub.get(d.id, {}).get(('floor', H))
        return pe is not None and pe[1] >= f[1]

    def go_offline(self, d, n):
        if not (self.rules.undo_revive or self.rules.compact or self.rules.compact_nofloor):
            return False
        if self.away(d) or n <= 0:
            return False
        d.off = self.now + n
        return True

    # ------------------------------------------------------------ purge of originals

    def purge_ok(self, d, H):
        if H not in self.orig or H in d.files or self.ex(d, H):
            return False
        e = d.held.get(('del', H))
        if e is None or self.now - d.delst.get(H, self.now) < KEEP:
            return False
        if self.rules.purge_published:
            # rule change: the delete the device relies on must be in the shared folder
            if not any((p := self.pub.get(i)) and (pe := p.get(('del', H))) and pe[0] >= e[0]
                       for i in self.pub):
                return False
        return True

    def purge_begin(self, d):
        if not d.rec:
            return False
        if self.rules.purge_lock:
            if d.cand is not None:
                return False                  # its round is still in progress
            return self.purge_begin_lock(d)
        self.mergeall(d)
        d.cand = set(H for H in self.files if self.purge_ok(d, H))
        return True

    def purge_end(self, d):
        if d.cand is None:
            return False
        for H in d.cand:
            if self.purge_ok(d, H):
                self.orig.discard(H)
                COUNTS['originals_purged'] += 1
            if self.rules.purge_lock and self.locks.get(H) == d.id:
                del self.locks[H]
        d.cand = None
        return True

    def purge_begin_lock(self, d):
        # Rule change: the purger first writes a purge marker for H in the shared folder,
        # then merges everything, then removes the original, then removes the marker.
        want = set()
        for H in self.files:
            if self.purge_ok(d, H) and H not in self.locks:
                if not self.rules.nopins and any(x[0] == H for x in self.pins):
                    COUNTS['pin_stops_purge'] += 1
                    continue
                self.locks[H] = d.id
                want.add(H)
        self.mergeall(d)
        d.cand = want
        return True

    # ------------------------------------------------------------ join and restore

    def strip_files(self, d):
        """Files that go into a copy of the device (join or backup)."""
        if self.rules.task_keep:
            # rule change: a file is dropped only when H does not exist in the view
            return set(H for H in d.files if self.ex(d, H))
        prot = set()
        for _, created in d.tasks:
            prot |= created
        return set(H for H in d.files if H not in prot and self.ex(d, H))

    def all_published(self, d):
        p = self.pub.get(d.id, {})
        return all((cur := p.get(k)) is not None and cur[0] >= e[0] for k, e in d.own.items())

    def publish_all_steps(self, d):
        r = [self.pin(d), self.clear(d), self.upload(d), self.publish(d)]
        return any(r)

    def snapshot(self, d):
        if not d.rec:
            return False
        if self.rules.copy_published:
            # V1: a copy is made only at the end of a publish of the source device
            ch = self.publish_all_steps(d)
            if not self.all_published(d):
                return ch
        self.snapseq += 1
        d.snap = (dict(d.held), dict(d.own), self.strip_files(d), dict(d.delst), d.clk,
                  self.snapseq)
        return True

    def restore(self, d):
        if d.snap is None:
            return False
        if self.rules.restore_newid and len(self.ids) >= MAXIDS:
            return False
        held, own, files, delst, clk, _ = d.snap
        self.dropped.pop(d.id, None)          # what it dropped is gone with its database
        disk = d.files | d.left
        if self.rules.keep_deleted:
            # rule change: the file of a Delete Permanently is still on the disk as long
            # as the device has not published its delete(H)
            p = self.pub.get(d.id, {})
            for H in d.stash:
                e = d.own.get(('del', H))
                cur = p.get(('del', H))
                if e is not None and (cur is None or cur[0] < e[0]):
                    disk.add(H)
        d.held, d.own, d.files, d.delst = dict(held), dict(own), set(files), dict(delst)
        d.tasks, d.done, d.stash, d.cand = [], [], {}, None
        d.cleared = set()
        if self.rules.disk_restore:
            # V2: the files on the disk stay; the device holds those its view shows
            d.files = set(H for H in disk if self.ex(d, H))
            d.left = disk - d.files
        if self.rules.restore_merge_all:
            d.blocked = 2                    # V3: no local action before it merged everything
        COUNTS['restores'] += 1
        if self.rules.restore_newid:
            # rule change: a restored device continues under a new device id, like a
            # device that joins from a copy; the old id never writes again.
            old = d.id
            self.pins = set(x for x in self.pins if x[1] != old)
            for H in [H for H, o in self.locks.items() if o == old]:
                del self.locks[H]
            d.id = 'd%d' % (len(self.ids) + 1)
            self.ids.append(d.id)
            d.own = dict(d.held) if self.rules.join_republish else {}
        if self.rules.purge_lock:
            for H in [H for H, o in self.locks.items() if o == d.id]:
                del self.locks[H]            # a restored device clears its own markers
            self.pins = set(x for x in self.pins if x[1] != d.id)
        if self.rules.restore_merge:
            d.blocked = True                 # rule change: no local action before the merge
        self.mark_lost()
        return True

    def disklost(self, d):
        """V2: the disk of a device is lost. The device joins again as a new device from
        its newest copy (entries and items, no files), or empty when it has no copy."""
        if not self.rules.disk_restore or not d.rec or len(self.ids) >= MAXIDS:
            return False
        if self.rules.nodisklost:
            return False
        old = d.id
        self.dropped.pop(old, None)
        self.pins = set(x for x in self.pins if x[1] != old)
        for H in [H for H, o in self.locks.items() if o == old]:
            del self.locks[H]
        if d.snap is not None:
            held, own, files, delst, clk, _ = d.snap
            d.held, d.delst, d.clk = dict(held), dict(delst), clk
        else:
            d.held, d.delst, d.clk = {}, {}, (0, 0)
        d.own = dict(d.held) if self.rules.join_republish else {}
        d.files, d.left, d.tasks, d.done, d.stash, d.cand = set(), set(), [], [], {}, None
        d.cleared, d.blocked, d.snap = set(), False, None
        d.id = 'd%d' % (len(self.ids) + 1)
        self.ids.append(d.id)
        COUNTS['disks_lost'] += 1
        self.mark_lost()
        return True

    def mergeall(self, d):
        if not d.rec:
            return False
        # One merge of everything in the shared folder; files are dropped or taken back
        # only according to the view after the whole merge.
        ch = False
        for src in list(self.pub):
            ch = self.merge(d, src, None, False) or ch
        if d.blocked == 2:
            d.blocked = False
            ch = True
        if ch:
            self.gc(d)
        return ch

    def cleanup(self, d):
        """Variant: the device deletes the files on its disk that its view does not show."""
        if not self.rules.orphan_cleanup or not d.left:
            return False
        d.left = set()
        return True

    def mark_lost(self):
        """After a restore: a logged action is lost when no device holds an entry of its
        key with its time or a later one and no such entry is published."""
        stores = [x.held for x in self.devs] + list(self.pub.values())
        for i, a in enumerate(self.log):
            if i in self.lost:
                continue
            key, t = a[1], a[2]
            for st in stores:
                e = st.get(key)
                if e is not None and e[0] >= t:
                    break
            else:
                self.lost.add(i)

    def join(self, src, use_snap):
        if len(self.devs) >= MAXDEV or len(self.ids) >= MAXIDS or not src.rec:
            return False
        if use_snap:
            if src.snap is None:
                return False
            held, _, files, delst, clk, _ = src.snap
        else:
            if self.rules.copy_published:
                ch = self.publish_all_steps(src)
                if not self.all_published(src):
                    return ch
            held, files, delst, clk = src.held, self.strip_files(src), src.delst, src.clk
        if self.rules.disk_restore:
            files = ()                        # V2: a copy holds entries and items, no files
        d = Dev('d%d' % (len(self.ids) + 1))
        d.held = dict(held)
        d.own = dict(held) if self.rules.join_republish else {}
        d.files = set(files)
        d.delst = dict(delst)
        d.clk = clk
        d.skew = src.skew
        self.devs.append(d)
        self.ids.append(d.id)
        COUNTS['joins'] += 1
        return True

    # ------------------------------------------------------------ dispatch

    def apply(self, a):
        k = a[0]
        self.begin_action()
        if k == 'wait':
            self.now += a[1]
            return True
        if a[1] >= len(self.devs):
            return False
        d = self.devs[a[1]]
        if k == 'jump':
            if d.skew == a[2]:
                return False
            d.skew = a[2]
            return True
        if k == 'start':
            return self.start(d)
        if d.blocked and k in LOCAL:
            return False
        if k in NETWORK and self.away(d):
            return False
        if k == 'compact':
            return self.compact(d)
        if k == 'offline':
            return self.go_offline(d, a[2])
        if k == 'import':
            return self.do_import(d, a[2], a[3])
        if k == 'delete':
            return self.do_delete(d, a[2])
        if k == 'undo':
            return self.do_undo(d, a[2])
        if k == 'edit':
            return self.do_edit(d, a[2], a[3], a[4])
        if k == 'tag':
            return self.do_tag(d, a[2], a[3], a[4])
        if k == 'folder':
            return self.do_folder(d, a[2], a[3], a[4])
        if k == 'tagdel':
            return self.do_tagdel(d, a[2])
        if k == 'folderdel':
            return self.do_folderdel(d, a[2])
        if k == 'tbegin':
            return self.task_begin(d, a[2])
        if k == 'tend':
            return self.task_end(d, a[2])
        if k == 'tundo':
            return self.task_undo(d, a[2])
        if k == 'tundo_after':
            return self.task_undo_after(d, a[2])
        if k == 'publish':
            return self.publish(d)
        if k == 'pubkeys':
            # a publish that is interrupted after it wrote the files of these keys
            return self.publish(d, lambda key: key in a[2])
        if k == 'pubpart':
            # a publish that is interrupted: each key is written or not by a coin
            # that the action's number decides, so a replay writes the same keys
            r = random.Random(a[2])
            chosen = {}
            return self.publish(d, lambda key: chosen.setdefault(key, r.random() < 0.5))
        if k == 'upload':
            return self.upload(d)
        if k == 'pin':
            return self.pin(d)
        if k == 'clear':
            return self.clear(d)
        if k == 'pubfull':                   # the publish steps of one device in a row
            r = [self.pin(d), self.clear(d), self.upload(d), self.publish(d)]
            return any(r)
        if k == 'merge':
            if a[2] >= len(self.ids):
                return False
            return self.merge(d, self.ids[a[2]], a[3])
        if k == 'fetch':
            return self.fetch(d, a[2])
        if k == 'purge_begin':
            return self.purge_begin(d)
        if k == 'purge_end':
            return self.purge_end(d)
        if k == 'snapshot':
            return self.snapshot(d)
        if k == 'restore':
            return self.restore(d)
        if k == 'join':
            return self.join(d, a[2])
        if k == 'ipost':
            return self.import_post(d, a[2], a[3], a[4])
        if k == 'group':
            return self.group(d, a[2], a[3], a[4])
        if k == 'ungroup':
            return self.ungroup(d, a[2])
        if k == 'detach':
            return self.detach(d, a[2])
        if k == 'cdelete':
            return self.cdelete(d, a[2])
        if k == 'reorder':
            return self.reorder(d, a[2], a[3])
        if k == 'cedit':
            return self.cedit(d, a[2], a[3], a[4])
        if k == 'ctag':
            return self.ctag(d, a[2], a[3], a[4])
        if k == 'cfolder':
            return self.cfolder(d, a[2], a[3], a[4])
        if k == 'mergeall':
            return self.mergeall(d)
        if k == 'disklost':
            return self.disklost(d)
        if k == 'cleanup':
            return self.cleanup(d)
        raise ValueError(a)

    # ------------------------------------------------------------ final phase and checks

    def finish(self):
        """Everything is delivered and fetched."""
        for d in self.devs:
            d.off = 0
        for d in self.devs:
            if not d.rec:
                self.begin_action()
                self.start(d)
            while d.tasks:
                self.begin_action()
                self.task_end(d, 0)
            if d.cand is not None:
                self.purge_end(d)
        for _ in range(4):
            for d in self.devs:
                self.pin(d)
                self.clear(d)
                self.upload(d)
                self.publish(d)
            for d in self.devs:
                self.mergeall(d)
            for d in self.devs:
                for H in self.files:
                    self.fetch(d, H)

    def check_revival(self, H, acts, dels, td, fmax, vs, bad):
        """The undo of a deletion brings back what any device had done before
        the deletion, unless a floor ended it. For each field, tag and folder, the
        latest action counts when it is earlier than the undone deletion, later than
        every floor, and no other deletion lies between it and the undone deletion."""
        def counts(a):
            return a[2] < td and a[2] > fmax and not any(a[2] < x[2] < td for x in dels)
        for fi, f in enumerate(SCALARS):
            eds = [a for a in acts if a[0] == 'edit' and a[1] == (f, H)]
            if eds:
                last = max(eds, key=lambda a: a[2])
                if counts(last) and any(x is None or x[fi] != last[3] for x in vs):
                    bad.append('undo-missed-an-earlier-field-edit')
        for kind, names, ik, dk, idx in (('tag', TAGS, 'it', 'td', 4),
                                         ('folder', FOLDERS, 'fi', 'fd', 5)):
            for T in names:
                its = [a for a in acts if a[1] == (ik, H, T)]
                if not its:
                    continue
                last = max(its, key=lambda a: a[2])
                if not counts(last):
                    continue
                tds = [a for a in acts if a[1] == (dk, T)]
                if last[3] == 'remove':
                    want = False
                elif kind == 'tag':
                    want = not any(a[2] >= last[2] for a in tds)
                else:
                    want = not tds
                if any(x is None or (T in x[idx]) != want for x in vs):
                    bad.append('undo-missed-an-earlier-' + kind)

    def finish_compact(self):
        """Every cut becomes old enough, every device compacts, and everything is
        delivered again."""
        self.now += 200
        for d in self.devs:
            self.begin_action()
            self.compact(d)
        self.finish()

    def final_check(self):
        self.finish()
        bad = self.check()
        if self.rules.compact or self.rules.compact_nofloor:
            self.finish_compact()
            bad += [x + '-after-compaction' for x in self.check()]
        return bad

    def check(self):
        devs, ids = self.devs, self.ids
        full = [self.view(d) for d in devs]
        views = [v[0] for v in full]
        cviews = [v[1] for v in full]
        hf = [frozenset(H for H in d.files if H in v) for d, v in zip(devs, views)]
        bad = []
        if any(v != full[0] for v in full[1:]):
            bad.append('P1-view')
        if any(f != hf[0] for f in hf[1:]):
            bad.append('P1-files')
        if self.rules.compact or self.rules.compact_nofloor:
            # What compaction dropped changes nothing: every device shows the view
            # of all entries that were ever held, published or dropped.
            union = {}
            stores = [x.held for x in devs] + list(self.pub.values()) + \
                list(self.dropped.values())
            for st in stores:
                for k, e in st.items():
                    cur = union.get(k)
                    if k[0] == 'floor':
                        if cur is None or (e[1], e[0]) > (cur[1], cur[0]):
                            union[k] = e
                    elif cur is None or e[0] > cur[0]:
                        union[k] = e
            uview = self.view_held(union)
            if any(v != uview for v in full):
                bad.append('dropped-entries-changed-the-library')
        # Actions whose entry a restore destroyed before it was published do not count.
        acts = [a for i, a in enumerate(self.log) if i not in self.lost]
        tdacts = {}
        for a in acts:
            if a[0] in ('td', 'fd'):
                tdacts.setdefault(a[1], []).append(a)

        def tagfolder(X, vs, adds, dels, dmax, pre, addkind):
            """P2 and P5 for the tags and folders of X (a file or a collection)."""
            for kind, names, ik, dk, idx in (('tag', TAGS, 'it', 'td', 4),
                                             ('folder', FOLDERS, 'fi', 'fd', 5)):
                for T in names:
                    its = [a for a in acts if a[1] == (ik, X, T)]
                    tds = tdacts.get((dk, T), ())
                    if not dels and not tds and not any(a[3] == 'remove' for a in its):
                        if adds and (its or any(T in a[3][idx] for a in adds)):
                            if not all(v is not None and T in v[idx] for v in vs):
                                bad.append('P2-' + pre + kind)
                    its2 = [a for a in its if a[2] > dmax]
                    if its2:
                        last = max(its2, key=lambda a: a[2])
                        if last[3] == 'remove':
                            if any(v is not None and T in v[idx] for v in vs):
                                bad.append('P5-' + pre + kind)
                        else:
                            killed = bool(tds) if kind == 'folder' else \
                                any(a[2] > last[2] for a in tds)
                            if not killed and any(v is not None and T not in v[idx] for v in vs):
                                bad.append('P5-' + pre + kind)
                    if kind == 'tag' and CHECK_TAGDEL and tds:
                        # optional reading of P5: a tag deletion is not undone by the start
                        # of recording on another device.
                        tdmax = max(a[2] for a in tds)
                        later = [a for a in acts if a[2] > tdmax and (
                            a[1] == (ik, X, T) and a[3] == 'add' or
                            a[0] == addkind and a[1][1] == X and T in a[3][4]
                            and a[5] != 'start' or
                            # the record of an undo counts like the record of an add
                            a[0] == 'undo' and addkind == 'add' and a[1][1] == X
                            and T in a[3][2][4])]
                        if not later and any(v is not None and T in v[4] for v in vs):
                            bad.append('P5-' + pre + 'tagdel')

        colls_on = self.rules.collections
        if colls_on:
            acttime = {}
            for a in acts:
                if a[2] > acttime.get(a[6], ZERO):
                    acttime[a[6]] = a[2]
            cdacts = [a for a in acts if a[0] == 'cdact']
            cdkeys = set(a[3] for a in cdacts)
        for H in self.files:
            adds = [a for a in acts if a[0] == 'add' and a[1][1] == H]
            dels = [a for a in acts if a[0] == 'del' and a[1][1] == H]
            undos = [a for a in acts if a[0] == 'undo' and a[1][1] == H]
            fmax = max((a[3] for a in acts if a[0] == 'floor' and a[1][1] == H), default=ZERO)
            # The cut by the log: the latest deletion or undo decides, and no cut is
            # earlier than the largest floor.
            last_reg = max(dels + undos, key=lambda a: a[2], default=None)
            if last_reg is None:
                dmax = ZERO
            elif last_reg[0] == 'undo':
                dmax = last_reg[3][1]
            else:
                dmax = last_reg[2]
            dmax = max(dmax, fmax)
            amax = max((a[2] for a in adds + undos), default=ZERO)
            delmax = max((a[2] for a in dels), default=ZERO)
            if adds and not dels and not all(H in f for f in hf):
                bad.append('P2-file')
            if adds or dels:
                if amax > delmax:
                    if not all(H in v for v in views):
                        bad.append('P3-exist')
                elif any(H in v for v in views):
                    bad.append('P3-exist')
            for fi, f in enumerate(SCALARS):
                eds = [a for a in acts if a[0] == 'edit' and a[1] == (f, H) and a[2] > dmax]
                if eds:
                    last = max(eds, key=lambda a: a[2])
                    if any(H in v and v[H][fi] != last[3] for v in views):
                        bad.append('P3-scalar')
            vs = [v.get(H) for v in views]
            # The undo is also checked when it is written as an add, so that the search
            # shows what the rules without undo_revive lose.
            undo_adds = [a for a in adds if isinstance(a[5], tuple) and a[5][0] == 'undo']
            last_undo = max(dels + undos + undo_adds, key=lambda a: a[2], default=None)
            if last_undo is not None and last_undo[0] != 'del':
                td = last_undo[5] if last_undo[0] == 'undo' else last_undo[5][1]
                self.check_revival(H, acts, dels, td, fmax, vs, bad)
            # A later add of the same device replaces the record of an earlier one, so
            # only the newest add per key can still hold a tag or a folder.
            newest_adds = {}
            for a in adds:
                if a[1] not in newest_adds or a[2] > newest_adds[a[1]][2]:
                    newest_adds[a[1]] = a
            if not dels:
                tagfolder(H, vs, list(newest_adds.values()), dels, dmax, '', 'add')
            else:
                # after a deletion only P5 applies, and only to views in which H exists
                tagfolder(H, [x for x in vs if x is not None], list(newest_adds.values()), dels,
                          dmax, '', 'add')
            if any(H in v for v in views) and H not in self.orig \
                    and not all(H in d.files for d in devs):
                bad.append('P4')
            if not colls_on:
                continue
            macts = [a for a in acts if a[0] == 'mem' and a[1][1] == H]
            joined = set(a[3] for a in macts if a[3] is not None) | \
                set(a[3][6] for a in adds if a[3][6] is not None)
            # P2 membership: a membership that nobody removed is not lost
            if joined and not dels and not any(a[3] is None for a in macts) \
                    and not (joined & cdkeys):
                if any(H in v and v[H][6] is None for v in views):
                    bad.append('P2-member')
            # P3 membership: the latest membership action after the latest deletion decides;
            # without one, the earliest add that names a collection decides.
            if any(H in v for v in views):
                after = [a for a in macts if a[2] > dmax]
                if after:
                    best = max(after, key=lambda a: (acttime[a[6]], a[2]))
                    exp, jt = best[3], best[2]
                else:
                    newest = {}               # a later add of the same key replaced the older
                    for a in adds:
                        if a[1] not in newest or a[2] > newest[a[1]][2]:
                            newest[a[1]] = a
                    la = sorted((a for a in newest.values()
                                 if a[2] > dmax and a[3][6] is not None), key=lambda a: a[2])
                    exp, jt = (la[0][3][6], la[0][2]) if la else (None, None)
                    if last_reg is not None and last_reg[0] == 'undo' and \
                            not any(a[2] > dmax for a in newest.values()):
                        # no add counts: the record of the undo counts as one
                        exp, jt = last_reg[3][2][6], last_reg[2]
                if exp is not None and self.rules.coll_delreg and any(
                        a[3] == exp and a[2] >= jt for a in cdacts):
                    exp = None
                if any(H in v and v[H][6] != exp for v in views):
                    bad.append('P3-member')
        if colls_on:
            for K in sorted(self.colls):
                kv = [c.get(K) for c in cviews]
                if all(x is None for x in kv):
                    live_views = []
                else:
                    live_views = [x for x in kv if x is not None]
                cadds = [a for a in acts if a[0] == 'cadd' and a[1][1] == K]
                # files that were ever put into K, and whether one of them was deleted
                kfiles = set(a[1][1] for a in acts if a[0] == 'mem' and a[3] == K) | \
                    set(a[1][1] for a in acts if a[0] == 'add' and a[3][6] == K)
                kdel = K in cdkeys or any(a[0] == 'del' and a[1][1] in kfiles for a in acts)
                for fi, f in enumerate(SCALARS):
                    eds = [a for a in acts if a[0] == 'edit' and a[1] == (f, K)]
                    if eds:
                        last = max(eds, key=lambda a: a[2])
                        if any(x[fi] != last[3] for x in live_views):
                            bad.append('P3-cscalar')
                    elif f in EMPTY and not kdel and any(a[3][fi] != EMPTY[f] for a in cadds):
                        if any(x[fi] == EMPTY[f] for x in live_views):
                            bad.append('P2-cfield')
                if live_views:
                    tagfolder(K, live_views, cadds, ('deleted',) if kdel else (), ZERO, 'c',
                              'cadd')
                    ords = [a for a in acts if a[0] == 'ord' and a[1][1] == K]
                    if ords:
                        last = max(ords, key=lambda a: a[2])
                        for x in live_views:
                            want = [H for H in last[3] if H in x[6]]
                            got = [H for H in x[6] if H in want]
                            if want != got:
                                bad.append('P3-order')
                # P3 for the deletion of a collection: no later joining, then K is gone
                cds = [a for a in cdacts if a[3] == K]
                if cds:
                    tc = max(acttime[a[6]] for a in cds)
                    later = any(a[2] > tc and (a[0] == 'mem' and a[3] == K or
                                               a[0] == 'add' and a[3][6] == K or
                                               a[0] == 'undo' and a[3][2][6] == K)
                                for a in acts)
                    if not later and live_views:
                        bad.append('P3-colldel')
            if CHECK_VANISH:
                alive = set(a[6] for a in acts)
                for K, members, aid in self.shown:
                    if aid not in alive or K in cdkeys:
                        continue
                    ok = True
                    for H in members:
                        if any(a[0] == 'del' and a[1][1] == H for a in acts) or any(
                                a[0] == 'mem' and a[1][1] == H and a[3] != K for a in acts):
                            ok = False
                    if ok and not all(K in c for c in cviews):
                        bad.append('P2-vanish')
        return bad


CHECK_VANISH = False
CHECK_TAGDEL = False
import collections
COUNTS = collections.Counter()
LOCAL = frozenset(('import', 'delete', 'undo', 'edit', 'tag', 'folder', 'tagdel', 'folderdel',
                   'tbegin', 'tend', 'tundo', 'tundo_after', 'ipost', 'group', 'ungroup',
                   'detach', 'cdelete', 'reorder', 'cedit', 'ctag', 'cfolder', 'cleanup',
                   'compact'))
NETWORK = frozenset(('publish', 'pubpart', 'pubkeys', 'upload', 'pin', 'clear', 'pubfull', 'merge', 'mergeall', 'fetch',
                     'purge_begin', 'purge_end', 'snapshot', 'join'))
DROPPABLE = frozenset(('add', 'name', 'notes', 'rating', 'status', 'it', 'fi'))


# ---------------------------------------------------------------- formatting

def fmt_time(t):
    return '(%d,%d,%s)' % t


def fmt_rec(r):
    base = r[-1] == 'baseline'
    if base:
        r = r[:-1]
    x = '%s/%s/%s/%s/tags=%s/folders=%s' % (r[0] or '-', r[1] or '-', r[2], r[3],
                                             ','.join(sorted(r[4])) or '-',
                                             ','.join(sorted(r[5])) or '-')
    if len(r) > 6 and r[6] is not None:
        x += '/collection=%s' % r[6]
    return x + ('/baseline' if base else '')


def fmt_key(key, value):
    k = key[0]
    if k == 'add':
        return 'add(%s,%s)=[%s]' % (key[1], key[2], fmt_rec(value))
    if k == 'cadd':
        return 'collection_add(%s,%s)=[%s]' % (key[1], key[2], fmt_rec(value))
    if k == 'mem':
        return 'member(%s)=%s' % (key[1], value or 'loose')
    if k == 'ord':
        return 'order(%s)=%s' % (key[1], ','.join(value))
    if k == 'cdel':
        return 'collection_delete(%s)' % key[1]
    if k == 'del':
        if isinstance(value, tuple) and value[0] == 'u':
            return 'undo(%s, before=%s)=[%s]' % (key[1], fmt_time(value[1]), fmt_rec(value[2]))
        return 'delete(%s)' % key[1]
    if k == 'floor':
        return 'floor(%s)=%s' % (key[1], fmt_time(value) if isinstance(value, tuple) else value)
    if k == 'it':
        return 'item_tag(%s,%s)=%s' % (key[1], key[2], value)
    if k == 'fi':
        return 'folder_item(%s,%s)=%s' % (key[1], key[2], value)
    if k == 'td':
        return 'tag_delete(%s)' % key[1]
    if k == 'fd':
        return 'folder_delete(%s)' % key[1]
    return '%s(%s)=%s' % (k, key[1], value)


def fmt_action(w, a):
    k = a[0]
    if k == 'wait':
        return 'world time advances by %d' % a[1]
    dn = w.devs[a[1]].id if a[1] < len(w.devs) else '?'
    if k == 'pubfull':
        return '%s runs all its publish steps' % dn
    if k == 'jump':
        return '%s wall clock skew becomes %+d' % (dn, a[2])
    if k == 'offline':
        return '%s is offline for %d units of world time' % (dn, a[2])
    if k == 'compact':
        return '%s compacts its entries' % dn
    if k == 'import':
        return '%s import %s [%s]' % (dn, a[2], fmt_rec(a[3]))
    if k == 'ipost':
        return '%s imports post %s with %s' % (dn, a[2], ', '.join(H for H, _ in a[3]))
    if k == 'group':
        return '%s groups %s as %s' % (dn, ', '.join(a[3]), a[2])
    if k == 'cdelete':
        return '%s deletes collection %s permanently' % (dn, a[2])
    if k == 'reorder':
        return '%s reorders %s to %s' % (dn, a[2], ','.join(a[3]))
    if k == 'tbegin':
        return '%s task begins with %s' % (dn, ', '.join('%s [%s]' % (H, fmt_rec(r))
                                                         for H, r in a[2]))
    if k == 'merge':
        src = w.ids[a[2]]
        if a[3] is None:
            return '%s merges all published entries of %s' % (dn, src)
        return '%s merges from %s only: %s' % (dn, src, ', '.join(
            fmt_key(x, '?') for x in a[3]))
    if k == 'join':
        return 'new device joins from %s of %s' % ('the backup' if a[2] else 'the current state', dn)
    return '%s %s' % (dn, ' '.join(str(x) for x in (k,) + a[2:]))


def show_state(w):
    out = []
    for d in w.devs:
        v, c = w.view(d)
        out.append('  %s view: %s; files on device: %s' % (
            d.id, '; '.join('%s=[%s]' % (H, fmt_rec(r)) for H, r in v.items() if r) or 'empty',
            ','.join(sorted(d.files)) or '-'))
        for K in sorted(c):
            out.append('     collection %s=[%s] members in order: %s' % (
                K, fmt_rec(c[K][:6]), ','.join(c[K][6])))
    out.append('  originals in shared folder: %s' % (','.join(sorted(w.orig)) or '-'))
    return '\n'.join(out)


# ---------------------------------------------------------------- running

def run(rules, init, nfiles, actions):
    w = World(rules, init, nfiles)
    for a in actions:
        w.apply(a)
    return w.final_check()


def replay_print(rules, init, nfiles, actions):
    w = World(rules, init, nfiles)
    for i, entry in enumerate(init):
        skew, pre = entry[0], entry[1]
        if pre is None:
            print('  init d%d: empty, recording, skew %+d' % (i + 1, skew))
        else:
            print('  init d%d: not recording, skew %+d, holds %s' % (
                i + 1, skew, ', '.join('%s [%s]' % (H, fmt_rec(r)) for H, r in pre)))
            for K, crec in (entry[2] if len(entry) > 2 else ()):
                print('           and collection %s [%s]' % (K, fmt_rec(crec)))
    n = 0
    for a in actions:
        w.tr = []
        text = fmt_action(w, a)
        nd = len(w.devs)
        if w.apply(a):
            n += 1
            if a[0] == 'join':
                text += ' as %s' % w.devs[-1].id
            if a[0] == 'restore' and w.rules.restore_newid or a[0] == 'disklost':
                text += ' and continues as %s' % w.devs[a[1]].id
            print('  %2d. %s%s' % (n, text, ('  -> writes ' + '; '.join(w.tr)) if w.tr else ''))
    w.tr = []
    w.finish()
    if w.tr:
        print('  final phase writes: ' + '; '.join(w.tr))
    print('  final phase: everything published, merged and fetched')
    print(show_state(w))
    bad = w.check()
    if w.rules.compact or w.rules.compact_nofloor:
        print('  violated before the final compaction:', sorted(set(bad)))
        w.tr = []
        w.finish_compact()
        if w.tr:
            print('  final compaction writes: ' + '; '.join(w.tr))
        print('  after every device compacted and everything was delivered again:')
        print(show_state(w))
        bad = [x + '-after-compaction' for x in w.check()]
    print('  violated:', sorted(set(bad)))


def shrink(rules, init, nfiles, actions, tag):
    def bad(i, acts):
        return tag in run(rules, i, nfiles, acts)
    actions = list(actions)
    init = list(init)
    changed = True
    while changed:
        changed = False
        i = len(actions) - 1
        while i >= 0:
            cand = actions[:i] + actions[i + 1:]
            if bad(init, cand):
                actions = cand
                changed = True
            i -= 1
        for j in range(len(init)):
            skew, pre = init[j][0], init[j][1]
            prec = init[j][2] if len(init[j]) > 2 else ()
            trials = []
            if pre is not None:
                trials.append((skew, None, ()))
                for x in range(len(pre)):
                    trials.append((skew, pre[:x] + pre[x + 1:], prec))
                if prec:
                    trials.append((skew, pre, ()))
                    trials.append((skew, tuple((H, r[:6] + (None,)) for H, r in pre), ()))
            if skew != 0:
                trials.append((0, pre, prec))
            for t in trials:
                cand = init[:j] + [t] + init[j + 1:]
                if bad(cand, actions):
                    init = cand
                    changed = True
                    break
        # try to turn partial merges into full merges and simplify records
        for i, a in enumerate(actions):
            if a[0] == 'merge' and a[3] is not None:
                cand = actions[:i] + [a[:3] + (None,)] + actions[i + 1:]
                if bad(init, cand):
                    actions = cand
                    changed = True
    # drop unused trailing devices
    while len(init) > 1:
        n = len(init) - 1
        if any(a[0] != 'wait' and (a[1] >= n or a[0] in ('merge',) and a[2] >= n)
               for a in actions):
            break
        if any(a[0] in ('join', 'restore', 'disklost') for a in actions):
            break
        if not bad(init[:-1], actions):
            break
        init = init[:-1]
    return tuple(init), actions


# ---------------------------------------------------------------- random search

def rand_crec(r):
    return (r.choice(NAMES), r.choice(NOTES), r.choice(RATINGS),
            r.choice(('inbox', 'inbox', 'active')),
            frozenset(T for T in TAGS if r.random() < 0.4),
            frozenset(F for F in FOLDERS if r.random() < 0.4))


def rand_rec(r, nfiles):
    return rand_crec(r) + (None,)


CKINDS = (('ipost', 9), ('group', 5), ('ungroup', 3), ('detach', 3), ('cdelete', 3),
          ('reorder', 2.5), ('cedit', 4), ('ctag', 5), ('cfolder', 2))


KINDS = (('import', 9), ('delete', 8), ('undo', 6), ('edit', 6), ('tag', 7), ('folder', 5),
         ('tagdel', 1.5), ('folderdel', 0.4), ('tbegin', 3), ('tend', 6), ('tundo', 5),
         ('tundo_after', 2), ('publish', 30), ('pin', 20), ('clear', 20), ('upload', 8),
         ('merge', 30), ('mergepart', 20), ('fetch', 14), ('join', 1), ('snapshot', 2),
         ('restore', 3), ('purge_begin', 7), ('purge_end', 8), ('wait', 5), ('jump', 1.5),
         ('start', 6), ('mergeall', 10), ('disklost', 1), ('cleanup', 2), ('compact', 6),
         ('offline', 2), ('pubpart', 10))
KIND_NAMES = [k for k, _ in KINDS]
KIND_W = [x for _, x in KINDS]
CKIND_NAMES = KIND_NAMES + [k for k, _ in CKINDS]
CKIND_W = KIND_W + [x for _, x in CKINDS]
SKEWS = (0, 0, 0, -6, 4, 25)


DENSE = False   # when set, one step may run all publish steps of a device in a row


def rand_action(w, r):
    """One random action. Some kinds look at the state to pick parameters that apply."""
    if DENSE and r.random() < 0.15:
        return ('pubfull', r.randrange(len(w.devs)))
    if w.rules.collections:
        k = r.choices(CKIND_NAMES, CKIND_W)[0]
    else:
        k = r.choices(KIND_NAMES, KIND_W)[0]
    n = len(w.devs)
    di = r.randrange(n)
    d = w.devs[di]
    F = w.files
    if k in ('ipost', 'group', 'ungroup', 'detach', 'cdelete', 'reorder', 'cedit', 'ctag',
             'cfolder'):
        items, colls = w.view(d)
        ks = sorted(colls)
        K = r.choice(ks) if ks else r.choice(POSTS)
        if k == 'ipost':
            hs = r.sample(F, r.randint(1, len(F)))
            return ('ipost', di, r.choice(POSTS), tuple((H, rand_rec(r, len(F))) for H in hs),
                    rand_crec(r))
        if k == 'group':
            loose = sorted(H for H, it in items.items() if it[6] is None)
            if len(loose) < 2:
                return ('group', di, 'g0', (), rand_crec(r))
            hs = r.sample(loose, r.randint(2, len(loose)))
            return ('group', di, 'g%d' % (w.gseq + 1), tuple(hs), rand_crec(r))
        if k == 'detach':
            ms = sorted(H for H, it in items.items() if it[6] is not None)
            return ('detach', di, r.choice(ms) if ms else r.choice(F))
        if k == 'reorder':
            lst = list(colls[K][6]) if K in colls else list(F)
            r.shuffle(lst)
            return ('reorder', di, K, tuple(lst))
        if k == 'cedit':
            f = r.choice(SCALARS)
            v = r.choice({'name': NAMES, 'notes': NOTES, 'rating': RATINGS,
                          'status': STATUSES}[f])
            return ('cedit', di, K, f, v)
        if k == 'ctag':
            return ('ctag', di, K, r.choice(TAGS), r.choice(('add', 'remove')))
        if k == 'cfolder':
            return ('cfolder', di, K, r.choice(FOLDERS), r.choice(('add', 'remove')))
        return (k, di, K)
    if k == 'wait':
        return ('wait', r.choice((1, KEEP, KEEP, KEEP + 1)))
    if k == 'offline':
        return ('offline', di, r.choice((KEEP + 1, 2 * KEEP, 4 * KEEP)))
    if k == 'pubpart':
        return ('pubpart', di, r.randrange(1 << 30))
    if k == 'jump':
        return ('jump', di, r.choice(SKEWS + (-15, 60)))
    if k == 'import':
        return ('import', di, r.choice(F), rand_rec(r, len(F)))
    if k == 'delete':
        hs = sorted(d.files)
        return ('delete', di, r.choice(hs) if hs else r.choice(F))
    if k == 'undo':
        hs = sorted(d.stash)
        return ('undo', di, r.choice(hs) if hs else r.choice(F))
    if k == 'fetch':
        ok = [(i, H) for i, x in enumerate(w.devs) for H in F
              if x.rec and H in w.orig and H not in x.files and w.ex(x, H)]
        if ok:
            i, H = r.choice(ok)
            return ('fetch', i, H)
        return ('fetch', di, r.choice(F))
    if k == 'edit':
        f = r.choice(SCALARS)
        v = r.choice({'name': NAMES, 'notes': NOTES, 'rating': RATINGS, 'status': STATUSES}[f])
        return ('edit', di, r.choice(F), f, v)
    if k == 'tag':
        return ('tag', di, r.choice(F), r.choice(TAGS), r.choice(('add', 'remove')))
    if k == 'folder':
        return ('folder', di, r.choice(F), r.choice(FOLDERS), r.choice(('add', 'remove')))
    if k == 'tagdel':
        return ('tagdel', di, r.choice(TAGS))
    if k == 'folderdel':
        return ('folderdel', di, r.choice(FOLDERS))
    if k == 'tbegin':
        hs = r.sample(F, r.randint(1, len(F)))
        return ('tbegin', di, tuple((H, rand_rec(r, len(F))) for H in hs))
    if k in ('tend', 'tundo', 'tundo_after'):
        return (k, di, r.choice((0, 0, 1)))
    if k in ('merge', 'mergepart'):
        srcs = [i for i, x in enumerate(w.ids) if w.pub.get(x)]
        if not srcs:
            return ('merge', di, 0, None)
        si = r.choice(srcs)
        if k == 'merge':
            return ('merge', di, si, None)
        return ('merge', di, si, tuple(x for x in w.pub[w.ids[si]] if r.random() < 0.5))
    if k == 'join':
        return ('join', di, r.random() < 0.3)
    return (k, di)


def gen_run(rules, seed):
    r = random.Random(seed)
    nfiles = r.choice((2, 2, 3))
    init = []
    for i in range(3):
        skew = r.choice(SKEWS)
        if r.random() < 0.4:
            pre = tuple((H, rand_rec(r, nfiles)) for H in ALL_FILES[:nfiles] if r.random() < 0.6)
            prec = ()
            if rules.collections and pre and r.random() < 0.6:
                # an older copy that already has a collection
                K = r.choice(('p1', 'g0'))
                pre = tuple((H, rec[:6] + (K,)) if r.random() < 0.7 else (H, rec)
                            for H, rec in pre)
                prec = ((K, rand_crec(r)),)
            init.append((skew, pre, prec))
        else:
            init.append((skew, None, ()))
    init = tuple(init)
    w = World(rules, init, nfiles)
    steps = r.randint(10, 40)
    actions = []
    tries = 0
    while len(actions) < steps and tries < steps * 30:
        tries += 1
        a = rand_action(w, r)
        if w.apply(a):
            actions.append(a)
    bad = w.final_check()
    saved = collections.Counter(COUNTS)
    if sorted(bad) != sorted(run(rules, init, nfiles, actions)):
        raise AssertionError('replay differs for seed %d' % seed)
    COUNTS.clear()
    COUNTS.update(saved)
    for a in actions:
        COUNTS['action ' + a[0]] += 1
    return init, nfiles, actions, bad


def worker(args):
    rules_on, lo, hi, tagdel, dense, vanish = args
    global CHECK_TAGDEL, DENSE, CHECK_VANISH
    CHECK_TAGDEL = tagdel
    DENSE = dense
    CHECK_VANISH = vanish
    rules = Rules(rules_on)
    found = {}
    counts = {}
    steps = 0
    COUNTS.clear()
    for seed in range(lo, hi):
        init, nfiles, actions, bad = gen_run(rules, seed)
        steps += len(actions)
        for tag in set(bad):
            counts[tag] = counts.get(tag, 0) + 1
            if len(found.setdefault(tag, [])) < 3:
                found[tag].append(seed)
    return counts, found, steps, dict(COUNTS)


def cmd_random(args):
    global CHECK_TAGDEL, DENSE, CHECK_VANISH
    CHECK_TAGDEL = args.tagdel
    DENSE = args.dense
    CHECK_VANISH = args.vanish
    on = tuple(x for x in args.rules.split(',') if x)
    rules = Rules(on)
    chunk = max(1, args.runs // (args.procs * 8))
    jobs = [(on, lo, min(lo + chunk, args.seed + args.runs), args.tagdel, args.dense,
             args.vanish)
            for lo in range(args.seed, args.seed + args.runs, chunk)]
    counts, found, steps = {}, {}, 0
    events = collections.Counter()
    with multiprocessing.Pool(args.procs) as pool:
        for c, f, s, ev in pool.imap_unordered(worker, jobs):
            events.update(ev)
            steps += s
            for k, v in c.items():
                counts[k] = counts.get(k, 0) + v
            for k, v in f.items():
                found.setdefault(k, []).extend(v)
    print('rules: %r' % rules)
    print('random runs: %d (seeds %d..%d), total steps before the final phase: %d'
          % (args.runs, args.seed, args.seed + args.runs - 1, steps))
    if args.stats:
        print('events: ' + ', '.join('%s=%d' % kv for kv in sorted(events.items())))
    if not counts:
        print('no violation found')
        return
    for tag in sorted(counts):
        print('%s: %d runs' % (tag, counts[tag]))
    printed = set()
    for tag in sorted(found):
        if args.only and tag not in args.only.split(','):
            continue
        best = None
        for seed in sorted(found[tag])[:args.shrink]:
            init, nfiles, actions, bad = gen_run(rules, seed)
            i2, a2 = shrink(rules, init, nfiles, actions, tag)
            if best is None or len(a2) < len(best[2]):
                best = (seed, i2, a2, nfiles)
        seed, i2, a2, nfiles = best
        if repr((i2, a2)) in printed:
            print('\n=== %s: same trace as above (seed %d) ===' % (tag, seed))
            continue
        printed.add(repr((i2, a2)))
        print('\n=== %s, shortest shrunk trace (seed %d, %d steps) ===' % (tag, seed, len(a2)))
        replay_print(rules, i2, nfiles, a2)
        print('  raw:', repr((i2, nfiles, a2)))


# ---------------------------------------------------------------- exhaustive search

R_T = ('nA', '', 0, 'inbox', frozenset(('t1',)), frozenset(), None)
R_0 = ('nB', '', 0, 'inbox', frozenset(), frozenset(), None)
C_T = ('cA', '', 0, 'inbox', frozenset(('t1',)), frozenset())
C_0 = ('cB', '', 0, 'inbox', frozenset(), frozenset())


def exh_actions(full):
    acts = []
    for di in (0, 1):
        o = 1 - di
        acts += [('start', di), ('import', di, 'h1', R_T), ('import', di, 'h1', R_0),
                 ('delete', di, 'h1'), ('undo', di, 'h1'),
                 ('tag', di, 'h1', 't1', 'add'), ('tag', di, 'h1', 't1', 'remove'),
                 ('tagdel', di, 't1'), ('edit', di, 'h1', 'name', 'nC'),
                 ('publish', di), ('upload', di), ('pin', di), ('clear', di), ('pubfull', di),
                 ('merge', di, o, None),
                 ('fetch', di, 'h1'),
                 ('purge_begin', di), ('purge_end', di), ('compact', di)]
        if full:
            acts += [('tbegin', di, (('h1', R_T),)), ('tend', di, 0), ('tundo', di, 0),
                     ('snapshot', di), ('restore', di), ('merge', di, di, None),
                     ('merge', di, 2, None), ('join', di, False), ('mergeall', di),
                     ('disklost', di)]
    acts.append(('wait', KEEP))
    return acts


def exh_cactions():
    """Action set of the exhaustive search with collections: 2 devices, 2 files, 1 tag."""
    acts = []
    for di in (0, 1):
        acts += [('ipost', di, 'p1', (('h1', R_T), ('h2', R_0)), C_T),
                 ('ipost', di, 'p1', (('h1', R_0),), C_0),
                 ('ipost', di, 'p2', (('h2', R_0),), C_0),
                 ('import', di, 'h1', R_0), ('import', di, 'h2', R_0),
                 ('group', di, 'ga' if di == 0 else 'gb',
                  ('h1', 'h2') if di == 0 else ('h2', 'h1'), C_T),
                 ('ungroup', di, '@h1'), ('detach', di, 'h1'), ('detach', di, 'h2'),
                 ('cdelete', di, '@h1'), ('cdelete', di, '@h2'),
                 ('reorder', di, '@h1', ('h2', 'h1')), ('cedit', di, '@h1', 'name', 'nC'),
                 ('ctag', di, '@h1', 't1', 'add'), ('ctag', di, '@h1', 't1', 'remove'),
                 ('delete', di, 'h1'), ('undo', di, 'h1'), ('tagdel', di, 't1'),
                 ('pubfull', di), ('mergeall', di), ('start', di), ('compact', di)]
    return acts


G_T = ('nA', '', 0, 'inbox', frozenset(('t1',)), frozenset(), 'g0')
G_0 = ('nB', '', 0, 'inbox', frozenset(), frozenset(), 'g0')
EXH_CINITS = (
    ((0, None, ()), (0, None, ())),
    ((0, None, ()), (-6, None, ())),
    ((0, (('h1', G_T), ('h2', G_0)), (('g0', C_T),)), (0, None, ())),
    ((0, (('h1', G_T), ('h2', G_0)), (('g0', C_T),)),
     (4, (('h1', G_0), ('h2', R_0)), (('g0', C_0),))),
)
EXH_INITS = (
    ((0, None), (0, None)),
    ((0, None), (-6, None)),
    ((0, (('h1', R_T),)), (0, None)),
    ((0, (('h1', R_T),)), (4, (('h1', R_0),))),
)


def exh_worker(args):
    rules_on, init, first, depth, full, tagdel = args
    global CHECK_TAGDEL
    CHECK_TAGDEL = tagdel
    rules = Rules(rules_on)
    acts = exh_cactions() if rules.collections else exh_actions(full)
    w0 = World(rules, init, 2 if rules.collections else 1)
    if not w0.apply(first):
        return 0, {}
    seen = {}
    found = {}
    count = [0]

    def rec(w, seq, left):
        k = hash(w.key())                    # states are told apart by a 64-bit hash
        if seen.get(k, -1) >= left:
            return
        seen[k] = left
        count[0] += 1
        c = w.clone()
        for tag in set(c.final_check()):
            if tag not in found or len(found[tag]) > len(seq):
                found[tag] = list(seq)
        if left == 0:
            return
        for a in acts:
            n = w.clone()
            if n.apply(a):
                seq.append(a)
                rec(n, seq, left - 1)
                seq.pop()

    rec(w0, [first], depth - 1)
    return count[0], found


def cmd_exhaust(args):
    global CHECK_TAGDEL
    CHECK_TAGDEL = args.tagdel
    on = tuple(x for x in args.rules.split(',') if x)
    rules = Rules(on)
    acts = exh_cactions() if rules.collections else exh_actions(args.full)
    inits = EXH_CINITS if rules.collections else EXH_INITS
    jobs = [(on, init, a, args.depth, args.full, args.tagdel) for init in inits for a in acts]
    total, found = 0, {}
    with multiprocessing.Pool(args.procs) as pool:
        for (c, f), job in zip(pool.imap(exh_worker, jobs), jobs):
            total += c
            for tag, seq in f.items():
                if tag not in found or len(found[tag][1]) > len(seq):
                    found[tag] = (job[1], seq)
    print('rules: %r' % rules)
    print('exhaustive: 2 devices, %s, 1 tag, %d initial states, %d actions per step, '
          'sequences up to %d actions, %d distinct states checked'
          % ('2 files, collections' if rules.collections else '1 file', len(inits), len(acts),
             args.depth, total))
    if not found:
        print('no violation found')
    for tag in sorted(found):
        init, seq = found[tag]
        print('\n=== %s, shortest trace (%d steps) ===' % (tag, len(seq)))
        replay_print(rules, init, 2 if rules.collections else 1, seq)
        print('  raw:', repr((init, seq)))


E2 = ((0, None), (0, None))
RA = ('nA', '', 0, 'inbox', frozenset(), frozenset(), None)
RT = ('nA', '', 0, 'inbox', frozenset(('t1',)), frozenset(), None)
SCRIPTS = {
    # A purge that relies on a delete that was never published, then two restores.
    'unpublished_delete_purge': (E2, 1, [
        ('snapshot', 1), ('import', 1, 'h1', RA), ('pin', 1), ('clear', 1), ('upload', 1),
        ('publish', 1), ('merge', 0, 1, None), ('snapshot', 0), ('fetch', 0, 'h1'),
        ('delete', 0, 'h1'), ('wait', KEEP + 1), ('purge_begin', 0), ('purge_end', 0),
        ('restore', 0), ('restore', 1)]),
    # A purge round that starts between the marker check and the publish of an add.
    'purge_between_check_and_publish': (E2, 1, [
        ('import', 0, 'h1', RA), ('pin', 0), ('clear', 0), ('publish', 0),
        ('merge', 1, 0, None), ('fetch', 1, 'h1'), ('delete', 1, 'h1'), ('publish', 1),
        ('merge', 0, 1, None), ('wait', KEEP + 1), ('snapshot', 0),
        ('import', 0, 'h1', RA), ('pin', 0), ('clear', 0), ('purge_begin', 1),
        ('publish', 0), ('restore', 0), ('purge_end', 1)]),
    # An import that writes nothing because the view is stale, then an older delete arrives.
    'stale_duplicate_import': (((0, None), (0, None), (-6, None)), 1, [
        ('import', 0, 'h1', RA), ('pin', 0), ('clear', 0), ('upload', 0), ('publish', 0),
        ('merge', 1, 0, None), ('merge', 2, 0, None), ('fetch', 1, 'h1'),
        ('delete', 1, 'h1'), ('publish', 1), ('wait', 3),
        ('import', 2, 'h1', RT), ('merge', 2, 1, None)]),
    # A tag deletion, then another device starts to record an old copy that has the tag.
    'tag_delete_then_start': (((0, (('h1', RT),)), (0, (('h1', RT),))), 1, [
        ('start', 0), ('tagdel', 0, 't1'), ('wait', 2), ('start', 1)]),
    # Delete and undo on a device that had not seen another device's edit.
    'undo_drops_unseen_edit': (E2, 1, [
        ('import', 0, 'h1', RA), ('pin', 0), ('clear', 0), ('upload', 0), ('publish', 0),
        ('merge', 1, 0, None), ('fetch', 1, 'h1'), ('edit', 0, 'h1', 'name', 'nC'),
        ('wait', 2), ('delete', 1, 'h1'), ('undo', 1, 'h1')]),
}


E2C = ((0, None, ()), (0, None, ()))
SCRIPTS.update({
    # Two devices group the same two loose files at the same time, in a different order.
    'split_grouping': (E2C, 2, [
        ('import', 0, 'h1', RA), ('import', 0, 'h2', RA), ('pubfull', 0), ('mergeall', 1),
        ('fetch', 1, 'h1'), ('fetch', 1, 'h2'),
        ('group', 0, 'g1', ('h1', 'h2'), C_T), ('group', 1, 'g2', ('h2', 'h1'), C_0)]),
    # A device compacts, and an interrupted publish writes the file without its own
    # dropped edit but not the file with the floor. Then its disk is lost, and the
    # deleting device undoes its deletion.
    'partial_publish_before_floor': (((0, None, ()), (0, None, ()), (0, None, ())), 1, [
        ('import', 0, 'h1', RA), ('pubfull', 0), ('mergeall', 1), ('fetch', 1, 'h1'),
        ('edit', 0, 'h1', 'name', 'nC'), ('pubfull', 0), ('mergeall', 1),
        ('delete', 1, 'h1'), ('pubfull', 1), ('mergeall', 0), ('wait', KEEP),
        ('compact', 0), ('pubkeys', 0, (('name', 'h1'),)), ('disklost', 0),
        ('undo', 1, 'h1')]),
    # V2 with new id on restore: the restored device deletes the files its view does not
    # show before it has merged its own published add.
    'cleanup_before_merge': (E2C, 1, [
        ('import', 0, 'h1', RA), ('pubfull', 0), ('delete', 0, 'h1'), ('pubfull', 0),
        ('wait', KEEP + 1), ('snapshot', 1), ('purge_begin', 0), ('import', 1, 'h1', RA),
        ('pubfull', 1), ('restore', 1), ('cleanup', 1)]),
})


def cmd_trace(args):
    global CHECK_TAGDEL
    CHECK_TAGDEL = args.tagdel
    rules = Rules(tuple(x for x in args.rules.split(',') if x))
    init, nfiles, actions = SCRIPTS[args.name]
    print('scripted trace %s, rules: %r' % (args.name, rules))
    replay_print(rules, init, nfiles, actions)


def main():
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest='cmd', required=True)
    p = sub.add_parser('random')
    p.add_argument('--runs', type=int, default=20000)
    p.add_argument('--seed', type=int, default=0)
    p.add_argument('--procs', type=int, default=12)
    p.add_argument('--rules', default='')
    p.add_argument('--shrink', type=int, default=6)
    p.add_argument('--stats', action='store_true')
    p.add_argument('--only', default='')
    p.add_argument('--dense', action='store_true')
    p.add_argument('--vanish', action='store_true')
    p.add_argument('--tagdel', action='store_true')
    p = sub.add_parser('exhaust')
    p.add_argument('--depth', type=int, default=5)
    p.add_argument('--procs', type=int, default=12)
    p.add_argument('--rules', default='')
    p.add_argument('--full', action='store_true')
    p.add_argument('--tagdel', action='store_true')
    p = sub.add_parser('trace')
    p.add_argument('name', choices=sorted(SCRIPTS))
    p.add_argument('--rules', default='')
    p.add_argument('--tagdel', action='store_true')
    args = ap.parse_args()
    if args.cmd == 'random':
        cmd_random(args)
    elif args.cmd == 'trace':
        cmd_trace(args)
    else:
        cmd_exhaust(args)


if __name__ == '__main__':
    main()
