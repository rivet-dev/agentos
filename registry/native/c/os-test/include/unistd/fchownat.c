#include <unistd.h>
#ifdef fchownat
#undef fchownat
#endif
int (*foo)(int, const char *, uid_t, gid_t, int) = fchownat;
int main(void) { return 0; }
