#include <sys/stat.h>
#ifdef fstatat
#undef fstatat
#endif
int (*foo)(int, const char *restrict, struct stat *restrict, int) = fstatat;
int main(void) { return 0; }
