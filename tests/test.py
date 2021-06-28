import os
import shutil

def main():
    print("begin child.")
    with os.fdopen(3, mode='rb') as r:
        with os.fdopen(4, mode='wb') as w:
            shutil.copyfileobj(r, w)
    print("done child.")


if __name__ == '__main__':
    main()
