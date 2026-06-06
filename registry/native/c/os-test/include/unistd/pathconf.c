#include <unistd.h>
#ifdef pathconf
#undef pathconf
#endif
long (*foo)(const char *, int) = pathconf;
int main(void) { return 0; }
