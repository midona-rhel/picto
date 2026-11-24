#!/usr/bin/env python3
"""Executable model of the sync rules. Standard library only. Run with python3 -I.

Entry keys:
  ('add', H, d)  value = record (name, notes, rating, status, tags, folders)
  ('del', H)     value = None
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
KEEP = 5          # keep time before an original may be purged, in units of world time
MAXDEV = 5
MAXIDS = 9


class Rules:
    """Rule switches. All False / 'split' = the rules as given (adversarial reading)."""
    NAMES = ('upload_first', 'join_republish', 'task_keep', 'restore_merge', 'restore_newid',
             'purge_published', 'purge_lock', 'tagdel_baseline', 'nopins')

    def __init__(self, on=()):
        for n in self.NAMES:
            setattr(self, n, n in on)

    def __repr__(self):
        return 'Rules(%s)' % ','.join(n for n in self.NAMES if getattr(self, n))


# ---------------------------------------------------------------- view

def del_time(held, H):
    e = held.get(('del', H))
    return e[0] if e else ZERO


def exists(held, ids, H):
    dt = del_time(held, H)
    for i in ids:
        e = held.get(('add', H, i))
        if e is not None and e[0] > dt:
            return True
    return False


def item(held, ids, H, rules=None):
    """The item H as the view shows it, or None when H does not exist."""
    dt = del_time(held, H)
    live = []
    for i in ids:
        e = held.get(('add', H, i))
        if e is not None and e[0] > dt:
            live.append(e)
    if not live:
        return None
    live.sort()
    first = live[0][1]
    out = []
    for idx, f in enumerate(SCALARS):
        e = held.get((f, H))
        if e is not None and e[0] > dt:
            v = e[1]
        elif f in EMPTY:
            v = EMPTY[f]
            for _, r in live:
                if r[idx] != v:
                    v = r[idx]
                    break
        else:
            v = first[idx]
        out.append(v)
    tags = []
    for T in TAGS:
        e = held.get(('it', H, T))
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
                    if rules is not None and rules.tagdel_baseline and len(r) > 6:
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
        e = held.get(('fi', H, F))
        if e is not None and e[0] > dt:
            if e[1] == 'remove':
                continue
        else:
            if not any(F in r[5] for _, r in live):
                continue
        folders.append(F)
    out.append(frozenset(tags))
    out.append(frozenset(folders))
    return tuple(out)


# ---------------------------------------------------------------- state

class Dev:
    __slots__ = ('id', 'rec', 'pre', 'held', 'own', 'files', 'tasks', 'done', 'clk',
                 'skew', 'stash', 'delst', 'snap', 'cand', 'blocked', 'cleared')

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
        return d

    def key(self):
        return (self.id, self.rec, tuple(sorted(self.held.items())),
                tuple(sorted(self.own.items())), tuple(sorted(self.files)),
                tuple((tuple(sorted(f.items())), tuple(sorted(c))) for f, c in self.tasks),
                tuple(self.done), self.clk, self.skew, tuple(sorted(self.stash.items())),
                tuple(sorted(self.delst.items())), self.snap is not None and self.snap[5],
                None if self.cand is None else tuple(sorted(self.cand)), self.blocked,
                tuple(sorted(self.cleared)))


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
        for i, (skew, pre) in enumerate(init):
            d = Dev('d%d' % (i + 1))
            d.skew = skew
            if pre is not None:
                d.rec = False
                d.pre = dict(pre)
                d.files = set(d.pre)
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
        return w

    def key(self):
        return (tuple(d.key() for d in self.devs),
                tuple(sorted((k, tuple(sorted(v.items()))) for k, v in self.pub.items())),
                tuple(sorted(self.orig)), self.now, tuple(self.log),
                tuple(sorted(self.locks.items())), tuple(sorted(self.lost)),
                tuple(sorted(self.pins)))

    # ------------------------------------------------------------ clock and entries

    def tick(self, d):
        w = self.now + d.skew
        l, c = d.clk
        d.clk = (w, 0) if w > l else (l, c + 1)
        return (d.clk[0], d.clk[1], d.id)

    def write(self, d, key, value, kind, src=None):
        t = self.tick(d)
        e = (t, value)
        d.held[key] = e
        d.own[key] = e
        if key[0] == 'del':
            d.delst[key[1]] = self.now
        self.log.append((kind, key, t, value, d.id, src))
        if self.tr is not None:
            self.tr.append('%s@%s' % (fmt_key(key, value), fmt_time(t)))
        return t

    def ex(self, d, H):
        return exists(d.held, self.ids, H)

    def it(self, d, H):
        return item(d.held, self.ids, H, self.rules)

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

    # ------------------------------------------------------------ local actions

    def start(self, d):
        if d.rec:
            return False
        d.rec = True
        for H in sorted(d.pre):
            rec = d.pre[H]
            if self.rules.tagdel_baseline:
                rec = rec + ('baseline',)
            self.write(d, ('add', H, d.id), rec, 'add', 'start')
        return True

    def do_import(self, d, H, rec):
        if not d.rec or H not in self.files:
            return False
        if not self.ex(d, H):
            self.write(d, ('add', H, d.id), rec, 'add', 'import')
            d.files.add(H)
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
        self.write(d, ('del', H), None, 'del')
        d.stash[H] = it
        self.gc(d)
        return True

    def do_undo(self, d, H):
        if H not in d.stash:
            return False
        rec = d.stash.pop(H)
        self.write(d, ('add', H, d.id), rec, 'add', 'undo')
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
        return [k for k, e in d.own.items() if k[0] == 'add' and k[1] in d.files
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

    def publish(self, d):
        if not d.rec:
            return False
        ch = False
        if self.rules.upload_first and not self.rules.purge_lock:
            ch = self.upload(d)
        p = self.pub.get(d.id, {})
        for k, e in d.own.items():
            cur = p.get(k)
            if cur is not None and e[0] <= cur[0]:
                continue
            if k[0] == 'add' and k[1] in d.files:
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
        if self.rules.purge_lock:
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

    def merge(self, d, src, keys=None):
        if not d.rec:
            return False
        p = self.pub.get(src)
        if not p:
            if src == d.id and d.blocked:
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
            if cur is None or t > cur[0]:
                d.held[k] = e
                ch = True
                if k[0] == 'del':
                    d.delst[k[1]] = self.now
            if t[2] == d.id or self.rules.join_republish and k in d.own:
                cur = d.own.get(k)
                if cur is None or t > cur[0]:
                    d.own[k] = e
                    ch = True
        if src == d.id and keys is None and d.blocked:
            d.blocked = False
            ch = True
        if ch:
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
        for src in list(self.pub):
            self.merge(d, src)
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
        for src in list(self.pub):
            self.merge(d, src)
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

    def snapshot(self, d):
        if not d.rec:
            return False
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
        d.held, d.own, d.files, d.delst = dict(held), dict(own), set(files), dict(delst)
        d.tasks, d.done, d.stash, d.cand = [], [], {}, None
        d.cleared = set()
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
            held, files, delst, clk = src.held, self.strip_files(src), src.delst, src.clk
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
        raise ValueError(a)

    # ------------------------------------------------------------ final phase and checks

    def finish(self):
        """Everything is delivered and fetched."""
        for d in self.devs:
            if not d.rec:
                self.start(d)
            while d.tasks:
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
                for src in list(self.pub):
                    self.merge(d, src)
            for d in self.devs:
                for H in self.files:
                    self.fetch(d, H)

    def check(self):
        devs, ids = self.devs, self.ids
        views = []
        for d in devs:
            v = {}
            for H in self.files:
                it = self.it(d, H)
                if it is not None:
                    v[H] = it
            views.append(v)
        hf = [frozenset(H for H in d.files if H in v) for d, v in zip(devs, views)]
        bad = []
        if any(v != views[0] for v in views[1:]):
            bad.append('P1-view')
        if any(f != hf[0] for f in hf[1:]):
            bad.append('P1-files')
        # Actions whose entry a restore destroyed before it was published do not count.
        acts = [a for i, a in enumerate(self.log) if i not in self.lost]
        for H in self.files:
            adds = [a for a in acts if a[0] == 'add' and a[1][1] == H]
            dels = [a for a in acts if a[0] == 'del' and a[1][1] == H]
            dmax = max((a[2] for a in dels), default=ZERO)
            amax = max((a[2] for a in adds), default=ZERO)
            if adds and not dels and not all(H in f for f in hf):
                bad.append('P2-file')
            if adds or dels:
                if amax > dmax:
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
            for kind, names, ik, dk, idx in (('tag', TAGS, 'it', 'td', 4),
                                             ('folder', FOLDERS, 'fi', 'fd', 5)):
                for T in names:
                    its = [a for a in acts if a[1] == (ik, H, T)]
                    tds = [a for a in acts if a[1] == (dk, T)]
                    if not dels and not tds and not any(a[3] == 'remove' for a in its):
                        if adds and (its or any(T in a[3][idx] for a in adds)):
                            if not all(H in v and T in v[H][idx] for v in views):
                                bad.append('P2-' + kind)
                    its2 = [a for a in its if a[2] > dmax]
                    if its2:
                        last = max(its2, key=lambda a: a[2])
                        if last[3] == 'remove':
                            if any(H in v and T in v[H][idx] for v in views):
                                bad.append('P5-' + kind)
                        else:
                            killed = bool(tds) if kind == 'folder' else \
                                any(a[2] > last[2] for a in tds)
                            if not killed and any(H in v and T not in v[H][idx] for v in views):
                                bad.append('P5-' + kind)
                    if kind == 'tag' and CHECK_TAGDEL and tds:
                        # optional reading of P5: a tag deletion is not undone by the start
                        # of recording on another device.
                        tdmax = max(a[2] for a in tds)
                        later = [a for a in acts if a[2] > tdmax and (
                            a[1] == (ik, H, T) and a[3] == 'add' or
                            a[0] == 'add' and a[1][1] == H and T in a[3][4] and a[5] != 'start')]
                        if not later and any(H in v and T in v[H][4] for v in views):
                            bad.append('P5-tagdel')
            if any(H in v for v in views) and H not in self.orig \
                    and not all(H in d.files for d in devs):
                bad.append('P4')
        return bad


CHECK_TAGDEL = False
import collections
COUNTS = collections.Counter()
LOCAL = frozenset(('import', 'delete', 'undo', 'edit', 'tag', 'folder', 'tagdel', 'folderdel',
                   'tbegin', 'tend', 'tundo', 'tundo_after'))


# ---------------------------------------------------------------- formatting

def fmt_time(t):
    return '(%d,%d,%s)' % t


def fmt_rec(r):
    s = '%s/%s/%s/%s/tags=%s/folders=%s' % (r[0], r[1] or '-', r[2], r[3],
                                             ','.join(sorted(r[4])) or '-',
                                             ','.join(sorted(r[5])) or '-')
    return s + ('/baseline' if len(r) > 6 else '')


def fmt_key(key, value):
    k = key[0]
    if k == 'add':
        return 'add(%s,%s)=[%s]' % (key[1], key[2], fmt_rec(value))
    if k == 'del':
        return 'delete(%s)' % key[1]
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
    if k == 'import':
        return '%s import %s [%s]' % (dn, a[2], fmt_rec(a[3]))
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
        v = {H: w.it(d, H) for H in w.files}
        out.append('  %s view: %s; files on device: %s' % (
            d.id, '; '.join('%s=[%s]' % (H, fmt_rec(r)) for H, r in v.items() if r) or 'empty',
            ','.join(sorted(d.files)) or '-'))
    out.append('  originals in shared folder: %s' % (','.join(sorted(w.orig)) or '-'))
    return '\n'.join(out)


# ---------------------------------------------------------------- running

def run(rules, init, nfiles, actions):
    w = World(rules, init, nfiles)
    for a in actions:
        w.apply(a)
    w.finish()
    return w.check()


def replay_print(rules, init, nfiles, actions):
    w = World(rules, init, nfiles)
    for i, (skew, pre) in enumerate(init):
        if pre is None:
            print('  init d%d: empty, recording, skew %+d' % (i + 1, skew))
        else:
            print('  init d%d: not recording, skew %+d, holds %s' % (
                i + 1, skew, ', '.join('%s [%s]' % (H, fmt_rec(r)) for H, r in pre)))
    n = 0
    for a in actions:
        w.tr = []
        text = fmt_action(w, a)
        nd = len(w.devs)
        if w.apply(a):
            n += 1
            if a[0] == 'join':
                text += ' as %s' % w.devs[-1].id
            if a[0] == 'restore' and w.rules.restore_newid:
                text += ' and continues as %s' % w.devs[a[1]].id
            print('  %2d. %s%s' % (n, text, ('  -> writes ' + '; '.join(w.tr)) if w.tr else ''))
    w.tr = []
    w.finish()
    if w.tr:
        print('  final phase writes: ' + '; '.join(w.tr))
    print('  final phase: everything published, merged and fetched')
    print(show_state(w))
    print('  violated:', sorted(set(w.check())))


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
            skew, pre = init[j]
            trials = []
            if pre is not None:
                trials.append((skew, None))
                for x in range(len(pre)):
                    trials.append((skew, pre[:x] + pre[x + 1:]))
            if skew != 0:
                trials.append((0, pre))
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
        if any(a[0] in ('join', 'restore') for a in actions):
            break
        if not bad(init[:-1], actions):
            break
        init = init[:-1]
    return tuple(init), actions


# ---------------------------------------------------------------- random search

def rand_rec(r, nfiles):
    return (r.choice(NAMES), r.choice(NOTES), r.choice(RATINGS),
            r.choice(('inbox', 'inbox', 'active')),
            frozenset(T for T in TAGS if r.random() < 0.4),
            frozenset(F for F in FOLDERS if r.random() < 0.4))


KINDS = (('import', 9), ('delete', 8), ('undo', 6), ('edit', 6), ('tag', 7), ('folder', 5),
         ('tagdel', 1.5), ('folderdel', 0.4), ('tbegin', 3), ('tend', 6), ('tundo', 5),
         ('tundo_after', 2), ('publish', 30), ('pin', 20), ('clear', 20), ('upload', 8),
         ('merge', 30), ('mergepart', 20), ('fetch', 14), ('join', 1), ('snapshot', 2),
         ('restore', 3), ('purge_begin', 7), ('purge_end', 8), ('wait', 5), ('jump', 1.5),
         ('start', 6))
KIND_NAMES = [k for k, _ in KINDS]
KIND_W = [x for _, x in KINDS]
SKEWS = (0, 0, 0, -6, 4, 25)


DENSE = False   # when set, one step may run all publish steps of a device in a row


def rand_action(w, r):
    """One random action. Some kinds look at the state to pick parameters that apply."""
    if DENSE and r.random() < 0.15:
        return ('pubfull', r.randrange(len(w.devs)))
    k = r.choices(KIND_NAMES, KIND_W)[0]
    n = len(w.devs)
    di = r.randrange(n)
    d = w.devs[di]
    F = w.files
    if k == 'wait':
        return ('wait', r.choice((1, KEEP, KEEP, KEEP + 1)))
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
            init.append((skew, pre))
        else:
            init.append((skew, None))
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
    w.finish()
    bad = w.check()
    saved = collections.Counter(COUNTS)
    if sorted(bad) != sorted(run(rules, init, nfiles, actions)):
        raise AssertionError('replay differs for seed %d' % seed)
    COUNTS.clear()
    COUNTS.update(saved)
    for a in actions:
        COUNTS['action ' + a[0]] += 1
    return init, nfiles, actions, bad


def worker(args):
    rules_on, lo, hi, tagdel, dense = args
    global CHECK_TAGDEL, DENSE
    CHECK_TAGDEL = tagdel
    DENSE = dense
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
    global CHECK_TAGDEL, DENSE
    CHECK_TAGDEL = args.tagdel
    DENSE = args.dense
    on = tuple(x for x in args.rules.split(',') if x)
    rules = Rules(on)
    chunk = max(1, args.runs // (args.procs * 8))
    jobs = [(on, lo, min(lo + chunk, args.seed + args.runs), args.tagdel, args.dense)
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

R_T = ('nA', '', 0, 'inbox', frozenset(('t1',)), frozenset())
R_0 = ('nB', '', 0, 'inbox', frozenset(), frozenset())


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
                 ('purge_begin', di), ('purge_end', di)]
        if full:
            acts += [('tbegin', di, (('h1', R_T),)), ('tend', di, 0), ('tundo', di, 0),
                     ('snapshot', di), ('restore', di), ('merge', di, di, None),
                     ('merge', di, 2, None), ('join', di, False)]
    acts.append(('wait', KEEP))
    return acts


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
    acts = exh_actions(full)
    w0 = World(rules, init, 1)
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
        c.finish()
        for tag in set(c.check()):
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
    acts = exh_actions(args.full)
    jobs = [(on, init, a, args.depth, args.full, args.tagdel) for init in EXH_INITS for a in acts]
    total, found = 0, {}
    with multiprocessing.Pool(args.procs) as pool:
        for (c, f), job in zip(pool.imap(exh_worker, jobs), jobs):
            total += c
            for tag, seq in f.items():
                if tag not in found or len(found[tag][1]) > len(seq):
                    found[tag] = (job[1], seq)
    print('rules: %r' % rules)
    print('exhaustive: 2 devices, 1 file, 1 tag, %d initial states, %d actions per step, '
          'sequences up to %d actions, %d distinct states checked'
          % (len(EXH_INITS), len(acts), args.depth, total))
    if not found:
        print('no violation found')
    for tag in sorted(found):
        init, seq = found[tag]
        print('\n=== %s, shortest trace (%d steps) ===' % (tag, len(seq)))
        replay_print(rules, init, 1, seq)
        print('  raw:', repr((init, 1, seq)))


E2 = ((0, None), (0, None))
RA = ('nA', '', 0, 'inbox', frozenset(), frozenset())
RT = ('nA', '', 0, 'inbox', frozenset(('t1',)), frozenset())
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
