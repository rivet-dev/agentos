#include <sys/stat.h>
#ifdef stat
#undef stat
#endif
int (*foo)(const char *restrict, struct stat *restrict) = stat;
int main(void) { return 0; }
