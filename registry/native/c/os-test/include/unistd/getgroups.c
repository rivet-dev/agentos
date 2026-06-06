#include <unistd.h>
#ifdef getgroups
#undef getgroups
#endif
int (*foo)(int, gid_t []) = getgroups;
int main(void) { return 0; }
