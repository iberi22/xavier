#include <string.h>

int copy_checked(char *dst, char *src, int len, int max) {
    if (len > max) return -1;
    memcpy(dst, src, len);
    return 0;
}
