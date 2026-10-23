def main():
    i = 0
    total = 0
    while i < 1000000:
        i = i + 1
        total = total + 60 * 60 * 24 % 1000
        if i % 3 == 0:
            total = total + 1
        elif i % 3 == 1:
            total = total - 1
        else:
            total = total + 2
    print(total)
main()
