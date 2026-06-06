#include <unistd.h>
#ifdef ttyname
#undef ttyname
#endif
char *(*foo)(int) = ttyname;
int main(void) { return 0; }
