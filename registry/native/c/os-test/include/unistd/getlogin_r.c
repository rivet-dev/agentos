#include <unistd.h>
#ifdef getlogin_r
#undef getlogin_r
#endif
int (*foo)(char *, size_t) = getlogin_r;
int main(void) { return 0; }
