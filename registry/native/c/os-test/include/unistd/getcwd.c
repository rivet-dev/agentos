#include <unistd.h>
#ifdef getcwd
#undef getcwd
#endif
char *(*foo)(char *, size_t) = getcwd;
int main(void) { return 0; }
