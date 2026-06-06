/*[DC]*/
#include <devctl.h>
#ifdef posix_devctl
#undef posix_devctl
#endif
int (*foo)(int, int, void *restrict, size_t, int *restrict) = posix_devctl;
int main(void) { return 0; }
