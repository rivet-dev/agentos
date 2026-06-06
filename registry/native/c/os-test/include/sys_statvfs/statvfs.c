#include <sys/statvfs.h>
#ifdef statvfs
#undef statvfs
#endif
int (*foo)(const char *restrict, struct statvfs *restrict) = statvfs;
int main(void) { return 0; }
