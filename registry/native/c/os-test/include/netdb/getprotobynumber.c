#include <netdb.h>
#ifdef getprotobynumber
#undef getprotobynumber
#endif
struct protoent *(*foo)(int) = getprotobynumber;
int main(void) { return 0; }
