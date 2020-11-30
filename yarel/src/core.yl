class Iter {
    fn map(self, f) {
        return MapIter(self.__iter__(), f);
    }

    fn collect(self) {
        var ret = [];
        for v in self {
            ret.push(v);
        }
        return ret;
    }
}

class MapIter < Iter {
    fn __init__(self, iterable, func) {
        self.iterable = iterable;
        self.func = func;
    }

    fn __iter__(self) {
        return self;
    }

    fn __next__(self) {
        var next = self.iterable.__next__();
        if next == sentinel() {
            return sentinel();
        }
        return self.func(next);
    }
}
