#include <unistd.h>
#ifdef getuid
#undef getuid
#endif
uid_t (*foo)(void) = getuid;
int main(void) { return 0; }
