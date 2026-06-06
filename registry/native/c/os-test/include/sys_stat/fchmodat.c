#include <sys/stat.h>
#ifdef fchmodat
#undef fchmodat
#endif
int (*foo)(int, const char *, mode_t, int) = fchmodat;
int main(void) { return 0; }
