#include <unistd.h>
#ifdef getlogin
#undef getlogin
#endif
char *(*foo)(void) = getlogin;
int main(void) { return 0; }
