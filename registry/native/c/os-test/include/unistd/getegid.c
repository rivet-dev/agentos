#include <unistd.h>
#ifdef getegid
#undef getegid
#endif
gid_t (*foo)(void) = getegid;
int main(void) { return 0; }
