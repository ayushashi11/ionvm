import time

def is_munchausen(number, cache):
    n = number
    total = 0

    if n == 0:
        return False

    while n > 0:
        digit = n % 10
        total += cache[digit]
        if total > number:
            return False
        n //= 10

    return total == number

def main():
    N = 10000000
    cache = [0, 1, 4, 27, 256, 3125, 46656, 823543, 16777216, 387420489]

    start = time.time()
    for n in range(N):
        if is_munchausen(n, cache):
            print(n)
    end = time.time()
    print(f"Time elapsed: {end - start}")

if __name__ == "__main__":
    main()
