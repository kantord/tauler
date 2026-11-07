// shaders/common_PathTracing_Vertex.glsl from Erich Loftis's THREE.js PathTracing Renderer,
// https://github.com/erichlof/THREE.js-PathTracing-Renderer at
// f8ded116a3c674db552d4481e59c61d28cf81833, CC0 (LICENSE in src/components/site/room/vendor/).
// The upstream file as it is, with Unix line endings. Used unedited.
precision highp float;
precision highp int;

void main()
{
	gl_Position = vec4( position, 1.0 );
}
