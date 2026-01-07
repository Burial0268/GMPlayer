import { Disposable } from "@applemusic-like-lyrics/core";

export class GLProgram implements Disposable {
	private gl: WebGLRenderingContext | WebGL2RenderingContext;
	program: WebGLProgram;
	private vertexShader: WebGLShader;
	private fragmentShader: WebGLShader;
	readonly attrs: { [name: string]: number };
	constructor(
		gl: WebGLRenderingContext | WebGL2RenderingContext,
		vertexShaderSource: string,
		fragmentShaderSource: string,
		private readonly label = "unknown",
	) {
		this.gl = gl;
		this.vertexShader = this.createShader(gl.VERTEX_SHADER, vertexShaderSource);
		this.fragmentShader = this.createShader(
			gl.FRAGMENT_SHADER,
			fragmentShaderSource,
		);
		this.program = this.createProgram();

		// 属性位置在 createProgram 中已绑定，这里可以直接使用
		const attrs: { [name: string]: number } = {};
		// 假设我们已经绑定了 a_position 到位置 0，a_texCoord 到位置 1
		attrs["a_position"] = 0;
		attrs["a_texCoord"] = 1;

		this.attrs = attrs;
	}
	private createShader(type: number, source: string) {
		const gl = this.gl;
		const shader = gl.createShader(type);
		if (!shader) throw new Error("Failed to create shader");
		gl.shaderSource(shader, source);
		gl.compileShader(shader);
		if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
			const infoLog = gl.getShaderInfoLog(shader);
			gl.deleteShader(shader);
			throw new Error(
				`Failed to compile ${type === gl.VERTEX_SHADER ? 'vertex' : 'fragment'} shader for "${this.label}":\n${infoLog}`,
			);
		}
		return shader;
	}
	private createProgram() {
		const gl = this.gl;
		const program = gl.createProgram();
		if (!program) throw new Error("Failed to create program");
		gl.attachShader(program, this.vertexShader);
		gl.attachShader(program, this.fragmentShader);

		// 在链接之前绑定属性位置
		gl.bindAttribLocation(program, 0, "a_position");
		gl.bindAttribLocation(program, 1, "a_texCoord");

		gl.linkProgram(program);

		// 检查链接状态
		if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
			const linkErrLog = gl.getProgramInfoLog(program);
			const validationErrLog = gl.getProgramInfoLog(program);
			gl.deleteProgram(program);
			throw new Error(
				`Failed to link program "${this.label}":\n${linkErrLog}\n\nValidation Log:\n${validationErrLog}\n\nVertex Shader:\n${gl.getShaderSource(this.vertexShader)}\n\nFragment Shader:\n${gl.getShaderSource(this.fragmentShader)}`,
			);
		}

		gl.validateProgram(program);

		// 检查验证状态 (虽然链接失败时通常不会进行到这里，但保留以供全面检查)
		if (!gl.getProgramParameter(program, gl.VALIDATE_STATUS)) {
			const errLog = gl.getProgramInfoLog(program);
			gl.deleteProgram(program);
			throw new Error(
				`Failed to validate program "${this.label}":\n${errLog}`,
			);
		}

		return program;
	}
	use() {
		const gl = this.gl;
		gl.useProgram(this.program);
	}
	private notFoundUniforms: Set<string> = new Set();
	private warnUniformNotFound(name: string) {
		if (this.notFoundUniforms.has(name)) return;
		this.notFoundUniforms.add(name);
		console.warn(
			`Failed to get uniform location for program "${this.label}": ${name}`,
		);
	}
	setUniform1f(name: string, value: number) {
		const gl = this.gl;
		const location = gl.getUniformLocation(this.program, name);
		if (!location) this.warnUniformNotFound(name);
		else gl.uniform1f(location, value);
	}
	setUniform2f(name: string, value1: number, value2: number) {
		const gl = this.gl;
		const location = gl.getUniformLocation(this.program, name);
		if (!location) this.warnUniformNotFound(name);
		else gl.uniform2f(location, value1, value2);
	}
	setUniform1i(name: string, value: number) {
		const gl = this.gl;
		const location = gl.getUniformLocation(this.program, name);
		if (!location) this.warnUniformNotFound(name);
		else gl.uniform1i(location, value);
	}
	setUniform3f(name: string, v1: number, v2: number, v3: number) {
		const gl = this.gl;
		const location = gl.getUniformLocation(this.program, name);
		if (!location) this.warnUniformNotFound(name);
		else gl.uniform3f(location, v1, v2, v3);
	}
	dispose() {
		const gl = this.gl;
		gl.deleteShader(this.vertexShader);
		gl.deleteShader(this.fragmentShader);
		gl.deleteProgram(this.program);
	}
}

export class GLTexture implements Disposable {
	readonly tex: WebGLTexture;
	readonly width: number;
	readonly height: number;

	constructor(
		private gl: WebGLRenderingContext | WebGL2RenderingContext,
		dataOrWidth: ImageData | number,
		heightOrOptions?: number | { internalFormat?: number; format?: number; type?: number },
		options?: { internalFormat?: number; format?: number; type?: number }
	) {
		const texture = gl.createTexture();
		if (!texture) throw new Error("Failed to create texture");
		this.tex = texture;

		gl.bindTexture(gl.TEXTURE_2D, texture);

		let realHeight: number | undefined;
		let realOptions: { internalFormat?: number; format?: number; type?: number } | undefined;

		if (typeof heightOrOptions === 'number') {
			realHeight = heightOrOptions;
			realOptions = options;
		} else if (typeof heightOrOptions === 'object') {
			realOptions = heightOrOptions;
		}


		if (typeof dataOrWidth === 'number') {
			this.width = dataOrWidth;
			this.height = realHeight!;
			const internalFormat = realOptions?.internalFormat ?? (gl as WebGL2RenderingContext).RGBA;
			const format = realOptions?.format ?? (gl as WebGL2RenderingContext).RGBA;
			const type = realOptions?.type ?? gl.UNSIGNED_BYTE;

			gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
			gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
			gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
			gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);

			gl.texImage2D(
				gl.TEXTURE_2D,
				0,
				internalFormat,
				this.width,
				this.height,
				0,
				format,
				type,
				null
			);

		} else if (dataOrWidth instanceof ImageData) {
			this.width = dataOrWidth.width;
			this.height = dataOrWidth.height;
			gl.texImage2D(
				gl.TEXTURE_2D,
				0,
				gl.RGBA,
				gl.RGBA,
				gl.UNSIGNED_BYTE,
				dataOrWidth,
			);
			gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
			gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
			gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.MIRRORED_REPEAT);
			gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.MIRRORED_REPEAT);
		} else {
			throw new Error("Invalid arguments for GLTexture constructor.");
		}

		// 解绑纹理
		gl.bindTexture(gl.TEXTURE_2D, null);
	}

	bind() {
		this.gl.bindTexture(this.gl.TEXTURE_2D, this.tex);
	}

	dispose(): void {
		this.gl.deleteTexture(this.tex);
	}
}

export function createOffscreenCanvas(width: number, height: number) {
	if ("OffscreenCanvas" in window) return new OffscreenCanvas(width, height);
	const canvas = document.createElement("canvas");
	canvas.width = width;
	canvas.height = height;
	return canvas;
}