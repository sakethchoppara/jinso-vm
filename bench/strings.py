def main():
    s = ""
    i = 0
    while i < 100000:
        s = s + "x"
        i = i + 1
    print(s == s)
main()
