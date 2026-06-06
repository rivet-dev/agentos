#include <sys/stat.h>
#ifdef fstat
#undef fstat
#endif
int (*foo)(int, struct stat *) = fstat;
int main(void) { return 0; }
