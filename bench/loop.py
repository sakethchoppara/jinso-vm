def main():
    i = 0
    total = 0
    while i < 10000000:
        total = total + i % 7
        i = i + 1
    print(total)
main()
