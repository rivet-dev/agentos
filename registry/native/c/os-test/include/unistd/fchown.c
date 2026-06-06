#include <unistd.h>
#ifdef fchown
#undef fchown
#endif
int (*foo)(int, uid_t, gid_t) = fchown;
int main(void) { return 0; }
