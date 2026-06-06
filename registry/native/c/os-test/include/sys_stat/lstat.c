#include <sys/stat.h>
#ifdef lstat
#undef lstat
#endif
int (*foo)(const char *restrict, struct stat *restrict) = lstat;
int main(void) { return 0; }
