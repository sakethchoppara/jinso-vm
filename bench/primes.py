# Count the primes up to n with an odd-only Sieve of Eratosthenes.
# Index k stands for the odd number 2k + 1, so even numbers take no space or work.
# A line-for-line match of primes.jo; idiomatic slice assignment would hand most of the
# work to C and stop being the same algorithm.
def count_primes(n):
    size = (n - 1) // 2
    composite = []
    k = 0
    while k <= size:
        composite.append(False)
        k = k + 1
    # Cross off odd multiples of each odd prime p, starting at p * p, stepping by 2p
    # (which is p in index terms). Primes above sqrt(n) have nothing left to cross off.
    i = 1
    while (2 * i + 1) * (2 * i + 1) <= n:
        if not composite[i]:
            p = 2 * i + 1
            j = (p * p - 1) // 2
            while j <= size:
                composite[j] = True
                j = j + p
        i = i + 1
    count = 1  # 2, the only even prime
    k = 1
    while k <= size:
        if not composite[k]:
            count = count + 1
        k = k + 1
    return count
print(count_primes(2000000))
