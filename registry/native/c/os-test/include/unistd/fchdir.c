#include <unistd.h>
#ifdef fchdir
#undef fchdir
#endif
int (*foo)(int) = fchdir;
int main(void) { return 0; }
