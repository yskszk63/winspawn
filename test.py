import os
import shutil

def main():
    with os.fdopen(3, mode='rb') as r:
        with os.fdopen(4, mode='wb') as w:
            shutil.copyfileobj(r, w)


if __name__ == '__main__':
    main()
