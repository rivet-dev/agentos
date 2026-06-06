#include <sys/statvfs.h>
#ifdef fstatvfs
#undef fstatvfs
#endif
int (*foo)(int, struct statvfs *) = fstatvfs;
int main(void) { return 0; }
