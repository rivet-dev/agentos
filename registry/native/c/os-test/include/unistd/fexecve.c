#include <unistd.h>
#ifdef fexecve
#undef fexecve
#endif
int (*foo)(int, char *const [], char *const []) = fexecve;
int main(void) { return 0; }
