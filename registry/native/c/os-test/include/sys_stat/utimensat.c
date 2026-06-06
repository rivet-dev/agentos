#include <sys/stat.h>
#ifdef utimensat
#undef utimensat
#endif
int (*foo)(int, const char *, const struct timespec [2], int) = utimensat;
int main(void) { return 0; }
