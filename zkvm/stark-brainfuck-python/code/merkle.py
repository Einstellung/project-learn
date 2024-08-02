from hashlib import blake2b
from os import urandom
from binascii import hexlify
import pickle


class Merkle:
    def __init__(self, data_array):
        # calculate depth and next power of two
        self.num_leafs = len(data_array)
        if (self.num_leafs - 1) & self.num_leafs == 0:
            next_power_of_two = self.num_leafs
        else:
            next_power_of_two = self.num_leafs << 1
            while (next_power_of_two - 1) & next_power_of_two != 0:
                next_power_of_two = (next_power_of_two - 1) & next_power_of_two
        self.depth = 1
        while next_power_of_two >= 1 << self.depth:
            self.depth += 1
        self.depth -= 1

        # append salt to leafs
        self.leafs = [leaf for leaf in data_array]

        # make room for nodes
        self.nodes = [bytes([0]*32)] * (2 * next_power_of_two)

        # populate nodes with hash of leafs
        for i in range(len(self.leafs)):
            unsalted_bytes = pickle.dumps(self.leafs[i])
            self.nodes[next_power_of_two +
                       i] = blake2b(unsalted_bytes).digest()

        # populate nodes with merger of children, recursively
        i = next_power_of_two
        j = 2 * next_power_of_two
        while i > 0:
            self.nodes[i-1] = blake2b(bytes(self.nodes[j-2]) +
                                      bytes(self.nodes[j-1])).digest()
            i -= 1
            j -= 2

    def root(self):
        return self.nodes[1]

    def open(self, index):
        authentication_path = []
        index = (1 << self.depth) | index
        while index > 1:
            authentication_path += [self.nodes[index ^ 1]]
            index >>= 1
        return authentication_path

    @staticmethod
    def verify(root, index, path, element):
        running_hash = blake2b(pickle.dumps(element)).digest()
        for node in path:
            if index % 2 == 0:
                running_hash = blake2b(running_hash + node).digest()
            else:
                running_hash = blake2b(node + running_hash).digest()
            index >>= 1
        return running_hash == root
