#include <unistd.h>
#ifdef getgid
#undef getgid
#endif
gid_t (*foo)(void) = getgid;
int main(void) { return 0; }
