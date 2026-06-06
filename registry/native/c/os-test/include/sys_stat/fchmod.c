#include <sys/stat.h>
#ifdef fchmod
#undef fchmod
#endif
int (*foo)(int, mode_t) = fchmod;
int main(void) { return 0; }
